//! `media_file` 表实体 —— **顶层媒体服务自有**的图片登记表:收到图片先落一行(pending),
//! 下载队列按行推进,落盘后翻成 done(失败 failed + 原因)。
//!
//! 归档**内容寻址、盘上无后缀**:`md5` 即内容 md5(小写 32 位 hex),也是落盘文件名与主键;
//! 后缀/格式是这里的元数据(`claimed_ext` = 发端报的后缀,会谎;`format` = 下载字节魔数嗅探
//! 的真相)。要用图一律按 md5 走库/走 [`super::wait`],不再有「文件名带不带后缀」的歧义。
//! 无名来源先以 `u<md5(url)>` 临时键登记,下载完按字节 md5 改真值。

use sea_orm::entity::prelude::*;

/// `media_file` 行模型。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "media_file")]
pub struct Model {
    /// 内容 md5(小写 32 位 hex),即落盘文件名(无后缀),主键。
    /// 无名来源下载完成前是 `u<md5(url)>` 临时键。
    #[sea_orm(primary_key, auto_increment = false)]
    pub md5: String,
    /// 来源 URL(QQ 图床,会过期;重见同图时刷新成最近一次的)。
    pub url: String,
    /// 状态:`pending`(排队/下载中)/ `done` / `failed`。
    pub status: String,
    /// 失败原因(仅 `failed` 时有)。
    pub error: Option<String>,
    /// 文件字节数(落盘后记)。
    pub size: Option<i64>,
    /// 发端报的后缀(wire 文件名的扩展名,最近一次;无名来源为 `None`)。**会谎**,只作参考。
    pub claimed_ext: Option<String>,
    /// 实际格式(下载字节魔数嗅探:png/jpeg/gif/webp/bmp;认不出为 `None`)。
    pub format: Option<String>,
    /// 是否动图(下载/自愈时经 [`super::is_animated_image`] 嗅探;未嗅探过为 `None`)。
    pub animated: Option<bool>,
    /// 遇见次数(每次 ingest 命中 +1)。
    pub seen_count: i64,
    /// 首次遇见(库侧 `now()`)。
    pub created_at: DateTimeWithTimeZone,
    /// 上次遇见(每次 ingest 刷新)。
    pub last_seen: DateTimeWithTimeZone,
    /// 上次使用(wait 取到 / 重发 / WebUI 取图时刷新;从未用过为 `None`)。
    pub last_used: Option<DateTimeWithTimeZone>,
    /// 下载完成时间。
    pub done_at: Option<DateTimeWithTimeZone>,
}

/// 独立登记表,无外联关系。
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
