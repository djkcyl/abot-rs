//! 插件开关的持久化 —— 把 `EnabledSet` 的覆盖快照存进 `setting` 表的专用一行。
//!
//! 用一行 `(plugin_key="__switches__", key="enabled")` 存整份 [`EnabledOverrides`](nagisa::EnabledOverrides)
//! 的 jsonb。`main` 启动时 [`load_overrides`] 读出装入 app;控制台插件页改开关后 [`store_overrides`]
//! 写回。读不到或反序列化失败都退回空覆盖(全按各插件 `default_enable`)。

use nagisa::EnabledOverrides;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveValue::Set, DatabaseConnection, EntityTrait};

use crate::web::entity::setting;

/// 专用 setting 行的复合主键。
const PLUGIN_KEY: &str = "__switches__";
const KEY: &str = "enabled";

/// 从 `setting` 行读出持久化的开关覆盖。缺行或解析失败 → `EnabledOverrides::default()`。
pub async fn load_overrides(db: &DatabaseConnection) -> EnabledOverrides {
    let row = setting::Entity::find_by_id((PLUGIN_KEY.to_string(), KEY.to_string()))
        .one(db)
        .await;
    match row {
        Ok(Some(m)) => serde_json::from_value(m.value).unwrap_or_default(),
        _ => EnabledOverrides::default(),
    }
}

/// 把当前开关覆盖快照写回 `setting` 行(upsert)。
pub async fn store_overrides(
    db: &DatabaseConnection,
    ov: &EnabledOverrides,
) -> Result<(), String> {
    let value = serde_json::to_value(ov).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().fixed_offset();
    let am = setting::ActiveModel {
        plugin_key: Set(PLUGIN_KEY.to_string()),
        key: Set(KEY.to_string()),
        value: Set(value),
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
    Ok(())
}
