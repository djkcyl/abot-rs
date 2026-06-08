//! `AGroup` —— 一个群的「数据 API」句柄：包住一行 [`group::Model`] + 一份连接。
//! 与 [`AUser`](crate::data::user::AUser) 同形：句柄本身即 API，没有仓储/DAO 中间层。
//!
//! 群的可调项都收在一个 `jsonb` 的 `config` 列里（开关/限额/白名单 …），故按群定制不必
//! 频繁加列。本句柄给出读/写该 JSON 的便捷方法（顶层键粒度），写时只 `UPDATE` config 一列。

use nagisa::prelude::*;
use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, DatabaseConnection, Set};
use serde_json::Value;

use crate::data::entity::group;
use crate::data::util::get_or_insert;

/// 一个群的可变状态句柄：一行 [`group::Model`] + 一份共享连接。
#[derive(Clone, Debug)]
pub struct AGroup {
    /// 当前行模型（含 `config` JSON）。写配置的方法会就地同步它。
    pub model: group::Model,
    /// 共享连接句柄（内部 `Arc`，克隆廉价）。
    pub db: DatabaseConnection,
}

impl AGroup {
    /// 群号。
    pub fn gid(&self) -> i64 {
        self.model.gid
    }

    /// 按 `gid` 取群：命中即包成句柄；缺失则插一行默认值（config 取库侧缺省 `{}`）再返回。
    ///
    /// 取或建走共享的 [`get_or_insert`]（与 [`AUser::get`](crate::data::user::AUser::get)
    /// 同一条路径）：并发下插入撞主键时回读对方行。
    pub async fn get(db: &DatabaseConnection, gid: i64) -> Result<Self> {
        let (model, _fresh) = get_or_insert::<group::Entity, _>(
            db,
            gid,
            || group::ActiveModel { gid: Set(gid), config: NotSet, created_at: NotSet },
            "群",
        )
        .await?;
        Ok(Self { model, db: db.clone() })
    }

    /// 整份群配置（`config` JSON 的引用）。
    pub fn config(&self) -> &Value {
        &self.model.config
    }

    /// 读取 `config` 顶层某键的值（不存在返回 `None`）。
    pub fn get_config(&self, key: &str) -> Option<&Value> {
        self.model.config.get(key)
    }

    /// 写入 `config` 顶层某键并落库（只 `UPDATE` config 一列），同步 `self.model.config`。
    ///
    /// 若 `config` 当前不是 JSON 对象（理论上不应发生——库侧缺省是 `{}`），会先重置为 `{}`
    /// 再写入该键，避免在非对象上设键静默丢失。
    pub async fn set_config(&mut self, key: impl Into<String>, value: Value) -> Result<()> {
        // 在内存副本上改键。
        let mut cfg = self.model.config.clone();
        if !cfg.is_object() {
            cfg = Value::Object(Default::default());
        }
        if let Value::Object(map) = &mut cfg {
            map.insert(key.into(), value);
        }

        // 只更新 config 一列（其余字段 NotSet，不动）。
        let mut am: group::ActiveModel = self.model.clone().into();
        am.config = Set(cfg.clone());
        am.created_at = NotSet;
        let updated = am.update(&self.db).await.context("更新群配置")?;
        self.model = updated;
        Ok(())
    }
}
