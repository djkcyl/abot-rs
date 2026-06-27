//! 数据层小工具 —— 与具体表无关的通用助手。
//!
//! - [`get_or_insert`] —— 把「按主键取，缺则插一行默认值，并发撞主键时回读」这套
//!   get-or-create 模式抽成一个泛型函数,各处共用(`AUser::get` / `AGroup::get` …)。
//! - [`business_day`] / [`business_day_start`] —— 全 bot 统一的「每日重置」口径(凌晨 4 点)。

use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveTime};
use nagisa::prelude::*;
use sea_orm::{ActiveModelBehavior, ActiveModelTrait, EntityTrait, IntoActiveModel, PrimaryKeyTrait};

/// 全 bot「每日重置」的统一时刻:**凌晨 4 点**(0:00–3:59 归前一业务日)。
/// 任何「每日 X」(签到、转账限额、将来的排行/任务刷新)一律经本模块的
/// [`business_day`] / [`business_day_start`] 取日界,不要各自手搓。
pub const DAILY_RESET: NaiveTime = match NaiveTime::from_hms_opt(4, 0, 0) {
    Some(t) => t,
    // 编译期求值:时刻非法直接编译失败,运行期零成本、无需 expect。
    None => panic!("4:00:00 是合法时刻"),
};

/// 当前业务日:本地时刻回拨 [`DAILY_RESET`] 距 0 点的时长后取自然日——4 点前算前一天。
/// 签到去重/连签等「按日比较」用它。
pub fn business_day() -> NaiveDate {
    business_day_of(Local::now().fixed_offset())
}

/// 任意带时区时刻所属的业务日(同 [`business_day`] 口径,4 点前算前一天)。比较两个时刻是否跨过日界
/// (如「上次结算到现在是否过了 4 点重置点」)用它。
pub fn business_day_of(dt: DateTime<FixedOffset>) -> NaiveDate {
    (dt.with_timezone(&Local).naive_local() - (DAILY_RESET - NaiveTime::MIN)).date()
}

/// 当前业务日的起点时刻(业务日当天 [`DAILY_RESET`],带本地时区)。流水时间窗查询
/// (如「今日已转额」)用它。
pub fn business_day_start() -> DateTime<FixedOffset> {
    business_day()
        .and_time(DAILY_RESET) // 常量时刻,无 Option
        .and_local_timezone(Local)
        .single()
        // 夏令时防御:本机时区(中国)无夏令时,4 点恒无歧义。
        .expect("本地 4 点无歧义")
        .fixed_offset()
}

/// 「取或建」一行:先按主键 `pk` 查,命中即返回;缺失则用 `build` 造一行 `ActiveModel` 插入,
/// 插入成功返回新行;**并发**下若被别处抢先插了同一主键(`insert` 撞唯一键报错),回读该行返回。
///
/// 故无论谁抢先,都稳定返回一行、**不**向调用方抛主键冲突——这正是 `AUser::get` / 签到行
/// 等手写多份的 get-or-create 语义,在此统一。`what` 是用于错误上下文的人话表名(如「用户」)。
///
/// 返回 `(model, fresh)`:`fresh == true` 表示**本调用**真的插入了一行新数据(竞态败者走回读
/// 分支时为 `false`)。调用方据此决定是否只在首次注册时打日志等副作用(如 `AUser::get` 的
/// 「新用户注册」),不需要的直接忽略该布尔。
///
/// 类型参数 `E` 是实体,`A` 是其 `ActiveModel`;`pk` 是主键值(`E` 的主键 `ValueType`,可 `Clone`
/// 以便回读时复用)。
pub async fn get_or_insert<E, A>(
    db: &sea_orm::DatabaseConnection,
    pk: <E::PrimaryKey as PrimaryKeyTrait>::ValueType,
    build: impl FnOnce() -> A,
    what: &str,
) -> Result<(E::Model, bool)>
where
    E: EntityTrait,
    <E::PrimaryKey as PrimaryKeyTrait>::ValueType: Clone,
    A: ActiveModelTrait<Entity = E> + ActiveModelBehavior + Send,
    E::Model: IntoActiveModel<A>,
{
    if let Some(model) = E::find_by_id(pk.clone()).one(db).await.with_context(|| format!("查询{what}"))? {
        return Ok((model, false));
    }

    // 缺失:插一行默认值。并发下可能与别处撞主键 → 回读对方刚插的行(此时 fresh=false)。
    match build().insert(db).await {
        Ok(model) => Ok((model, true)),
        Err(_) => {
            let model = E::find_by_id(pk)
                .one(db)
                .await
                .with_context(|| format!("插入冲突后回读{what}"))?
                .with_context(|| format!("插入冲突后{what}仍不存在"))?;
            Ok((model, false))
        }
    }
}
