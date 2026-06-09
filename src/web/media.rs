//! 媒体图签名 —— `/api/media` 路由不鉴权（`<img>` 取图无从带 token），但 URL 带一段进程级密钥
//! 算出的签名，挡住对 `IMAGE_DIR` 的任意文件名枚举/伪造：拿不到签名就取不到图。
//!
//! 密钥是进程级随机、首次取用时生成，重启即换 → 旧签名失效。无碍：签名 URL 由审核详情每次现取、
//! 从不持久化，前端每次打开都拿到当下有效的链接。

use std::sync::OnceLock;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// 进程级随机密钥（首次取用时生成）。
fn secret() -> &'static [u8; 32] {
    static S: OnceLock<[u8; 32]> = OnceLock::new();
    S.get_or_init(|| std::array::from_fn(|_| rand::random::<u8>()))
}

/// 对文件名算签名：HMAC-SHA256 取前 8 字节的十六进制（16 字符，够防猜防伪）。
fn sig_for(name: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret()).expect("HMAC 接受任意长度密钥");
    mac.update(name.as_bytes());
    hex::encode(&mac.finalize().into_bytes()[..8])
}

/// 带签名的访问路径：`/api/media/<名>?sig=<签名>`。详情返图链接用它。
pub fn signed_path(name: &str) -> String {
    format!("/api/media/{name}?sig={}", sig_for(name))
}

/// 校验 (文件名, 签名) 是否匹配本进程密钥。
pub fn verify(name: &str, sig: &str) -> bool {
    sig_for(name) == sig
}
