//! DatabaseProvider —— 通用 DB 表管理。
//! `database` DataService 自动发现 public 下所有表 + 行数;`db/query` 带筛选/排序/搜索/分页读;
//! `db/insert`/`db/update`/`db/delete` 写。
//!
//! 安全约束(全程不可破):
//! 1. 表名、列名等**标识符绝不来自原始用户输入**:表名须命中 information_schema 实时清单,
//!    列名(出现在 SELECT/WHERE/ORDER BY/SET/INSERT/PK 任意位置)须命中该表 information_schema.columns
//!    的真实列集;不命中即报错。标识符一律加双引号 `"name"`。
//! 2. **所有值参数化**,从不拼接;走 `Statement::from_sql_and_values`,占位符 `$N`。
//! 3. 值统一**按文本或 NULL 绑定**,SQL 里把占位符 cast 到该列真实 Postgres 类型(`$1::int8`、
//!    `$2::jsonb`、`$3::timestamptz`),由 Postgres 自行把文本解析成对应类型。
//! 4. 筛选算子、排序方向取自固定白名单;limit 夹到 1..=200,offset≥0。

use nagisa::async_trait;
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement, Value as SqlValue};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::web::registry::{
    AuthUser, ConsoleContext, ConsolePlugin, ConsolePluginCtor, ConsoleRegistry, WebDataService, WebListener,
};

const MAX_LIMIT: u64 = 200;
const DEFAULT_LIMIT: u64 = 50;

/// 筛选算子白名单。`needs_value=false` 的两个不绑值。
const FILTER_OPS: &[&str] = &["=", "!=", "<", ">", "<=", ">=", "like", "ilike", "is null", "is not null"];

/// 一列的真实元信息(全部取自 information_schema)。
struct ColInfo {
    name: String,
    data_type: String,
    /// Postgres 底层类型名,用作占位符的 cast 目标(int8/jsonb/timestamptz/text…)。
    udt_name: String,
    nullable: bool,
}

pub struct DatabaseProvider {
    db: DatabaseConnection,
}
impl DatabaseProvider {
    pub fn new(cx: &ConsoleContext) -> Arc<Self> {
        Arc::new(Self { db: cx.db.clone() })
    }
}

impl ConsolePlugin for DatabaseProvider {
    fn register(self: Arc<Self>, reg: &mut ConsoleRegistry) {
        reg.add_data_service(Box::new(DbData(Arc::clone(&self))));
        reg.add_listener(Box::new(DbQuery(Arc::clone(&self))));
        reg.add_listener(Box::new(DbInsert(Arc::clone(&self))));
        reg.add_listener(Box::new(DbUpdate(Arc::clone(&self))));
        reg.add_listener(Box::new(DbDelete(self)));
    }
}

// ───────────────────────── 元数据发现 ─────────────────────────

/// public 下所有 BASE TABLE 的表名(已排序)。表名校验的唯一权威来源。
async fn discover_tables(db: &DatabaseConnection) -> Result<Vec<String>, String> {
    let stmt = Statement::from_string(
        db.get_database_backend(),
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema='public' AND table_type='BASE TABLE' ORDER BY table_name",
    );
    let rows = db.query_all(stmt).await.map_err(|e| e.to_string())?;
    Ok(rows.iter().filter_map(|r| r.try_get::<String>("", "table_name").ok()).collect())
}

/// 校验表名:必须命中实时清单,否则报错。返回原表名(已确认安全)。
async fn checked_table(db: &DatabaseConnection, table: &str) -> Result<String, String> {
    let tables = discover_tables(db).await?;
    if tables.iter().any(|t| t == table) { Ok(table.to_string()) } else { Err(format!("未知的表:{table}")) }
}

/// 某表的列元信息(按列序)。表名作为**参数**绑定,不拼进 SQL。
async fn table_columns(db: &DatabaseConnection, table: &str) -> Result<Vec<ColInfo>, String> {
    let stmt = Statement::from_sql_and_values(
        db.get_database_backend(),
        "SELECT column_name, data_type, udt_name, is_nullable \
         FROM information_schema.columns \
         WHERE table_schema='public' AND table_name=$1 ORDER BY ordinal_position",
        [SqlValue::from(table.to_string())],
    );
    let rows = db.query_all(stmt).await.map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .map(|r| ColInfo {
            name: r.try_get::<String>("", "column_name").unwrap_or_default(),
            data_type: r.try_get::<String>("", "data_type").unwrap_or_default(),
            udt_name: r.try_get::<String>("", "udt_name").unwrap_or_default(),
            nullable: r.try_get::<String>("", "is_nullable").unwrap_or_default() == "YES",
        })
        .collect())
}

/// 某表主键列(按键序)。无主键则空。表名作为参数绑定。
async fn pk_columns(db: &DatabaseConnection, table: &str) -> Result<Vec<String>, String> {
    let stmt = Statement::from_sql_and_values(
        db.get_database_backend(),
        "SELECT kcu.column_name FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name=kcu.constraint_name AND tc.table_schema=kcu.table_schema \
         WHERE tc.constraint_type='PRIMARY KEY' AND tc.table_schema='public' \
           AND tc.table_name=$1 ORDER BY kcu.ordinal_position",
        [SqlValue::from(table.to_string())],
    );
    let rows = db.query_all(stmt).await.map_err(|e| e.to_string())?;
    Ok(rows.iter().filter_map(|r| r.try_get::<String>("", "column_name").ok()).collect())
}

// ───────────────────────── 值绑定 ─────────────────────────

/// JSON 值 → 绑定文本(或 NULL)。配合 SQL 里的 `$N::<udt>` cast,由 Postgres 完成解析。
/// - JSON null → SQL NULL
/// - JSON 字符串 → 原文
/// - JSON 数/布尔 → 其字面文本
/// - JSON 对象/数组 → 其 JSON 序列化文本(供 jsonb 等列)
fn bind_text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

/// 把 `bind_text` 的结果包成 sea-orm `Value`:`Some` → 文本,`None` → SQL NULL。
fn text_value(t: Option<String>) -> SqlValue {
    SqlValue::from(t)
}

/// 在列集里找一列(校验列名白名单)。返回其元信息引用,不命中即报错。
fn checked_col<'a>(cols: &'a [ColInfo], name: &str) -> Result<&'a ColInfo, String> {
    cols.iter().find(|c| c.name == name).ok_or_else(|| format!("未知的列:{name}"))
}

/// 占位符 cast 的目标类型。数组列的 `udt_name` 形如 `_int4`,转成标准 `int4[]`(两种 Postgres 都认,
/// 取标准形更清晰);其余类型(含 jsonb、枚举、复合)原样。`udt_name` 取自系统目录,可直接拼。
fn cast_type(udt: &str) -> String {
    match udt.strip_prefix('_') {
        Some(base) => format!("{base}[]"),
        None => udt.to_string(),
    }
}

// ───────────────────────── DataService:表清单 ─────────────────────────

struct DbData(Arc<DatabaseProvider>);
#[async_trait]
impl WebDataService for DbData {
    fn key(&self) -> &'static str {
        "database"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn get(&self) -> Value {
        let db = &self.0.db;
        let names = match discover_tables(db).await {
            Ok(n) => n,
            Err(_) => return json!({ "tables": [] }),
        };
        let mut tables = Vec::with_capacity(names.len());
        for name in names {
            // 表名已命中清单,加引号安全;count 无参数。
            let sql = format!("SELECT count(*) AS n FROM \"{name}\"");
            let rows: i64 = match db.query_one(Statement::from_string(db.get_database_backend(), sql)).await {
                Ok(Some(row)) => row.try_get::<i64>("", "n").unwrap_or(0),
                _ => 0,
            };
            tables.push(json!({ "name": name, "rows": rows }));
        }
        json!({ "tables": tables })
    }
}

// ───────────────────────── db/query:读 ─────────────────────────

struct DbQuery(Arc<DatabaseProvider>);
#[async_trait]
impl WebListener for DbQuery {
    fn event(&self) -> &'static str {
        "db/query"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let db = &self.0.db;
        let backend = db.get_database_backend();
        let table_raw = args.get("table").and_then(|v| v.as_str()).ok_or("缺少 table")?;
        let table = checked_table(db, table_raw).await?;
        let cols = table_columns(db, &table).await?;
        if cols.is_empty() {
            return Err("该表没有列".to_string());
        }
        let pk = pk_columns(db, &table).await?;

        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0);

        // WHERE 子句 + 绑定值(占位符从 $1 起累加)。
        let mut clauses: Vec<String> = Vec::new();
        let mut values: Vec<SqlValue> = Vec::new();
        let mut idx = 0usize; // 已用占位符数

        // 筛选:每条 {column, op, value?}。
        if let Some(filters) = args.get("filters").and_then(|v| v.as_array()) {
            for f in filters {
                let col_name = f.get("column").and_then(|v| v.as_str()).ok_or("筛选缺少 column")?;
                let col = checked_col(&cols, col_name)?;
                let op = f.get("op").and_then(|v| v.as_str()).ok_or("筛选缺少 op")?;
                if !FILTER_OPS.contains(&op) {
                    return Err(format!("不支持的算子:{op}"));
                }
                match op {
                    "is null" | "is not null" => {
                        clauses.push(format!("\"{}\" {}", col.name, op.to_uppercase()));
                    }
                    _ => {
                        idx += 1;
                        // like/ilike 的值原样绑定(由调用方决定是否带 % 通配)。
                        let raw = f.get("value").unwrap_or(&Value::Null);
                        values.push(text_value(bind_text(raw)));
                        clauses.push(format!(
                            "\"{}\" {} ${}::{}",
                            col.name,
                            op.to_uppercase(),
                            idx,
                            cast_type(&col.udt_name)
                        ));
                    }
                }
            }
        }

        // 搜索:对所有列做 `"col"::text ILIKE $N`,组内 OR。
        if let Some(search) = args.get("search").and_then(|v| v.as_str())
            && !search.is_empty()
        {
            idx += 1;
            values.push(text_value(Some(format!("%{search}%"))));
            let ors: Vec<String> = cols.iter().map(|c| format!("\"{}\"::text ILIKE ${}", c.name, idx)).collect();
            clauses.push(format!("({})", ors.join(" OR ")));
        }

        let where_sql = if clauses.is_empty() { String::new() } else { format!(" WHERE {}", clauses.join(" AND ")) };

        // ORDER BY:仅当 order_by 命中白名单。方向只取 asc/desc。
        let mut order_sql = String::new();
        if let Some(ob) = args.get("order_by").and_then(|v| v.as_str()) {
            let oc = checked_col(&cols, ob)?;
            let dir = match args.get("order_dir").and_then(|v| v.as_str()) {
                Some("desc") => "DESC",
                _ => "ASC",
            };
            order_sql = format!(" ORDER BY \"{}\" {}", oc.name, dir);
        }

        let all_cols = cols.iter().map(|c| format!("\"{}\"", c.name)).collect::<Vec<_>>().join(", ");

        // 行:外层 to_jsonb 整行,内层做投影/筛选/排序/分页。limit/offset 已夹紧,可内联。
        let row_sql = format!(
            "SELECT to_jsonb(sub) AS row FROM \
             (SELECT {all_cols} FROM \"{table}\"{where_sql}{order_sql} LIMIT {limit} OFFSET {offset}) sub"
        );
        let row_stmt = Statement::from_sql_and_values(backend, &row_sql, values.clone());
        let rows = db.query_all(row_stmt).await.map_err(|e| e.to_string())?;
        let items: Vec<Value> = rows.iter().map(|r| r.try_get::<Value>("", "row").unwrap_or(Value::Null)).collect();

        // total:同样的 WHERE + 同样的绑定值(不含 limit/offset)。
        let count_sql = format!("SELECT count(*) AS n FROM \"{table}\"{where_sql}");
        let count_stmt = Statement::from_sql_and_values(backend, &count_sql, values);
        let total: i64 = match db.query_one(count_stmt).await.map_err(|e| e.to_string())? {
            Some(row) => row.try_get::<i64>("", "n").unwrap_or(0),
            None => 0,
        };

        let columns: Vec<Value> =
            cols.iter().map(|c| json!({ "name": c.name, "data_type": c.data_type, "nullable": c.nullable })).collect();
        Ok(json!({
            "columns": columns,
            "pk": pk,
            "rows": items,
            "total": total,
            "limit": limit,
            "offset": offset,
        }))
    }
}

// ───────────────────────── db/insert:增 ─────────────────────────

struct DbInsert(Arc<DatabaseProvider>);
#[async_trait]
impl WebListener for DbInsert {
    fn event(&self) -> &'static str {
        "db/insert"
    }
    fn authority(&self) -> u8 {
        5
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let db = &self.0.db;
        let backend = db.get_database_backend();
        let table_raw = args.get("table").and_then(|v| v.as_str()).ok_or("缺少 table")?;
        let table = checked_table(db, table_raw).await?;
        let cols = table_columns(db, &table).await?;
        let set = args.get("values").and_then(|v| v.as_object()).ok_or("缺少 values")?;
        if set.is_empty() {
            return Err("没有要写入的列".to_string());
        }

        let mut names: Vec<String> = Vec::new();
        let mut placeholders: Vec<String> = Vec::new();
        let mut values: Vec<SqlValue> = Vec::new();
        for (k, v) in set {
            let col = checked_col(&cols, k)?; // 列名白名单
            names.push(format!("\"{}\"", col.name));
            values.push(text_value(bind_text(v)));
            placeholders.push(format!("${}::{}", values.len(), cast_type(&col.udt_name)));
        }
        let sql = format!("INSERT INTO \"{table}\" ({}) VALUES ({})", names.join(", "), placeholders.join(", "));
        let stmt = Statement::from_sql_and_values(backend, &sql, values);
        db.execute(stmt).await.map_err(|e| e.to_string())?;
        tracing::warn!(target: "abot::web::audit", action = "insert", table = %table, "网页控制台数据库写操作");
        Ok(json!({ "ok": true }))
    }
}

// ───────────────────────── db/update:改 ─────────────────────────

/// 校验 pk 入参:表须有主键,且 pk 的键集与主键列**完全一致**。返回主键列(按真实键序)。
fn require_pk_match(pk_cols: &[String], pk_arg: &serde_json::Map<String, Value>) -> Result<(), String> {
    if pk_cols.is_empty() {
        return Err("无主键,无法定位行".to_string());
    }
    if pk_arg.len() != pk_cols.len() || !pk_cols.iter().all(|c| pk_arg.contains_key(c)) {
        return Err("主键列不匹配".to_string());
    }
    Ok(())
}

struct DbUpdate(Arc<DatabaseProvider>);
#[async_trait]
impl WebListener for DbUpdate {
    fn event(&self) -> &'static str {
        "db/update"
    }
    fn authority(&self) -> u8 {
        5
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let db = &self.0.db;
        let backend = db.get_database_backend();
        let table_raw = args.get("table").and_then(|v| v.as_str()).ok_or("缺少 table")?;
        let table = checked_table(db, table_raw).await?;
        let cols = table_columns(db, &table).await?;
        let pk_cols = pk_columns(db, &table).await?;

        let pk_arg = args.get("pk").and_then(|v| v.as_object()).ok_or("缺少 pk")?;
        require_pk_match(&pk_cols, pk_arg)?;
        let set = args.get("set").and_then(|v| v.as_object()).ok_or("缺少 set")?;
        if set.is_empty() {
            return Err("没有要更新的列".to_string());
        }

        let mut values: Vec<SqlValue> = Vec::new();
        // SET 段
        let mut set_parts: Vec<String> = Vec::new();
        for (k, v) in set {
            let col = checked_col(&cols, k)?;
            values.push(text_value(bind_text(v)));
            set_parts.push(format!("\"{}\"=${}::{}", col.name, values.len(), cast_type(&col.udt_name)));
        }
        // WHERE 段(按真实主键列序遍历,值取自 pk_arg)
        let mut where_parts: Vec<String> = Vec::new();
        for pc in &pk_cols {
            let col = checked_col(&cols, pc)?;
            let v = pk_arg.get(pc).unwrap_or(&Value::Null);
            values.push(text_value(bind_text(v)));
            where_parts.push(format!("\"{}\"=${}::{}", col.name, values.len(), cast_type(&col.udt_name)));
        }
        let sql = format!("UPDATE \"{table}\" SET {} WHERE {}", set_parts.join(", "), where_parts.join(" AND "));
        let stmt = Statement::from_sql_and_values(backend, &sql, values);
        let res = db.execute(stmt).await.map_err(|e| e.to_string())?;
        tracing::warn!(target: "abot::web::audit", action = "update", table = %table, "网页控制台数据库写操作");
        Ok(json!({ "ok": true, "affected": res.rows_affected() }))
    }
}

// ───────────────────────── db/delete:删 ─────────────────────────

struct DbDelete(Arc<DatabaseProvider>);
#[async_trait]
impl WebListener for DbDelete {
    fn event(&self) -> &'static str {
        "db/delete"
    }
    fn authority(&self) -> u8 {
        5
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let db = &self.0.db;
        let backend = db.get_database_backend();
        let table_raw = args.get("table").and_then(|v| v.as_str()).ok_or("缺少 table")?;
        let table = checked_table(db, table_raw).await?;
        let cols = table_columns(db, &table).await?;
        let pk_cols = pk_columns(db, &table).await?;

        let pk_arg = args.get("pk").and_then(|v| v.as_object()).ok_or("缺少 pk")?;
        require_pk_match(&pk_cols, pk_arg)?;

        let mut values: Vec<SqlValue> = Vec::new();
        let mut where_parts: Vec<String> = Vec::new();
        for pc in &pk_cols {
            let col = checked_col(&cols, pc)?;
            let v = pk_arg.get(pc).unwrap_or(&Value::Null);
            values.push(text_value(bind_text(v)));
            where_parts.push(format!("\"{}\"=${}::{}", col.name, values.len(), cast_type(&col.udt_name)));
        }
        let sql = format!("DELETE FROM \"{table}\" WHERE {}", where_parts.join(" AND "));
        let stmt = Statement::from_sql_and_values(backend, &sql, values);
        let res = db.execute(stmt).await.map_err(|e| e.to_string())?;
        tracing::warn!(target: "abot::web::audit", action = "delete", table = %table, "网页控制台数据库写操作");
        Ok(json!({ "ok": true, "affected": res.rows_affected() }))
    }
}

nagisa::inventory::submit! {
    ConsolePluginCtor(|cx: &ConsoleContext| -> Arc<dyn ConsolePlugin> { DatabaseProvider::new(cx) })
}
