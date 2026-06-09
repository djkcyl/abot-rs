//! 静态资源 —— 把前端构建产物(`web/dist/`)用 rust-embed 打进二进制;SPA history 兜底。
//!
//! `web/dist/` 是前端 `vite build` 产物;未构建时只有 `.gitkeep`,rust-embed 嵌空、
//! 回退到 `DIST_MISSING` 提示页。`index.html` 缺失即视为前端未构建。

use axum::http::{header, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Asset;

/// 前端未构建时的提示页。
const DIST_MISSING: &str =
    "<h1>abot 控制台</h1><p>前端未构建。请先 <code>cd web &amp;&amp; npm ci &amp;&amp; npm run build</code> 生成 <code>web/dist/</code>,再重启 abot。</p>";

/// 静态资源 + SPA 兜底:命中文件直接返回;未命中回退 index.html(history 路由);
/// index.html 也无 → 提示前端未构建。
pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(content) = Asset::get(path) {
        let mime = content.metadata.mimetype();
        return ([(header::CONTENT_TYPE, mime.to_owned())], content.data).into_response();
    }
    match Asset::get("index.html") {
        Some(content) => Html(content.data).into_response(),
        None => Html(DIST_MISSING).into_response(),
    }
}
