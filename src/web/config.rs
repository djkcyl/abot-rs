//! 配置框架 —— 插件经 `inventory` 注册 [`ConfigSpec`](schema + 默认 + 校验);[`ConfigStore`]
//! 持各插件当前配置(`ArcSwap`,启动从 `setting` 表/default 装载,`config/set` 写库后热替换)。
//! 插件运行期经 `State<ConfigStore>` 读当前值。

use arc_swap::ArcSwap;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveValue::Set, DatabaseConnection, EntityTrait};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::web::entity::setting;

/// 一个插件的配置登记:schema 给前端渲染表单,default 给初值,validate 校验+归一化提交值。
pub struct ConfigSpec {
    pub plugin_key: &'static str,
    pub title: &'static str,
    /// JSON Schema(schemars 生成),前端据此渲染表单。
    pub schema: fn() -> Value,
    /// 默认值(序列化自 `T::default()`)。
    pub default: fn() -> Value,
    /// 校验+归一化:把提交的 Value 反序列化进 T 再回序列化(非法即 Err)。
    pub validate: fn(&Value) -> Result<Value, String>,
}
nagisa::inventory::collect!(ConfigSpec);

/// 把 Value 当 `T` 校验并归一化(单态化成 `fn(&Value)->Result<Value,String>` 作 ConfigSpec.validate)。
pub fn validate_as<T: DeserializeOwned + Serialize>(v: &Value) -> Result<Value, String> {
    let parsed: T = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(parsed).map_err(|e| e.to_string())
}

/// 各插件当前配置的进程内存储:plugin_key → `Arc<ArcSwap<Value>>`。map 启动定型,只换 ArcSwap
/// 内的值。`Clone` 廉价(内部 `Arc`)。
#[derive(Clone)]
pub struct ConfigStore {
    inner: Arc<HashMap<&'static str, Arc<ArcSwap<Value>>>>,
}

impl ConfigStore {
    /// 启动装载:每个 ConfigSpec 当前值 = `setting`(plugin_key,'config')有则用,无则 default。
    pub async fn load(db: &DatabaseConnection) -> Result<Self, sea_orm::DbErr> {
        let mut map: HashMap<&'static str, Arc<ArcSwap<Value>>> = HashMap::new();
        for spec in nagisa::inventory::iter::<ConfigSpec> {
            let stored = setting::Entity::find_by_id((spec.plugin_key.to_string(), "config".to_string()))
                .one(db)
                .await?
                .map(|m| m.value);
            let value = stored.unwrap_or_else(|| (spec.default)());
            map.insert(spec.plugin_key, Arc::new(ArcSwap::from_pointee(value)));
        }
        Ok(Self { inner: Arc::new(map) })
    }

    /// 读某插件当前配置值。
    pub fn get(&self, plugin_key: &str) -> Option<Arc<Value>> {
        self.inner.get(plugin_key).map(|s| s.load_full())
    }

    /// 读并反序列化成 T。
    pub fn typed<T: DeserializeOwned>(&self, plugin_key: &str) -> Option<T> {
        let v = self.get(plugin_key)?;
        serde_json::from_value((*v).clone()).ok()
    }

    /// 写库 + 热替换(config/set 校验通过后调)。
    pub async fn set(&self, db: &DatabaseConnection, plugin_key: &str, value: Value) -> Result<(), String> {
        let now = chrono::Utc::now().fixed_offset();
        let am = setting::ActiveModel {
            plugin_key: Set(plugin_key.to_string()),
            key: Set("config".to_string()),
            value: Set(value.clone()),
            updated_at: Set(now),
        };
        setting::Entity::insert(am)
            .on_conflict(
                OnConflict::columns([setting::Column::PluginKey, setting::Column::Key])
                    .update_columns([setting::Column::Value, setting::Column::UpdatedAt])
                    .to_owned(),
            )
            .exec(db)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(slot) = self.inner.get(plugin_key) {
            slot.store(Arc::new(value));
        }
        Ok(())
    }
}
