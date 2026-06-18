//! 探针:对一个地址做现代 + 旧版 ping,打印原始状态 JSON 与解析出的字段。用来挨个看各类服务端
//! 的真实响应。
//!
//!   cargo run --example mc_probe -- 127.0.0.1:25700
//!   cargo run --example mc_probe -- mc.example.net --as 1.16   # 以 1.16 协议握手
//!   cargo run --example mc_probe -- mc.example.net --resolve   # 两段式,穿透 ViaProxy 占位
//!   cargo run --example mc_probe -- play.example.net --bedrock # 基岩版(RakNet/UDP,默认 19132)

use std::time::Duration;

use abot::integrations::minecraft::{self, PingOptions};

#[tokio::main]
async fn main() {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:25700".into());
    let t = Duration::from_secs(8);

    // --bedrock:走基岩版 RakNet/UDP ping(默认端口 19132)。
    if std::env::args().any(|a| a == "--bedrock") {
        println!("===== Bedrock ping {addr} =====");
        let opts = minecraft::BedrockOptions { timeout: t, attempts: 3 };
        match minecraft::ping_bedrock(&addr, &opts).await {
            Ok(r) => {
                let s = &r.status;
                println!("edition  : {}", s.edition);
                println!("version  : {:?}  protocol {}", s.version_name, s.protocol);
                println!("players  : {}/{}", s.online, s.max);
                println!("gamemode : {}  (id {:?})", s.gamemode, s.gamemode_id);
                println!("latency  : {:?}", s.latency);
                println!("motd1    : {:?}", s.motd_line1);
                println!("motd2    : {:?}", s.motd_line2);
                println!("ports    : v4={:?} v6={:?}  guid={}", s.port_v4, s.port_v6, s.server_guid);
                println!("raw      : {}", s.raw_motd);
            }
            Err(e) => println!("Bedrock 失败: {e}"),
        }
        return;
    }

    // --raw:只打印未截断的原始状态 JSON(抓真机样本用)。
    if std::env::args().any(|a| a == "--raw") {
        let opts = PingOptions { use_srv: true, allow_legacy: false, timeout: t, ..Default::default() };
        match minecraft::ping_with(&addr, &opts).await {
            Ok(r) => println!("{}", r.raw_json),
            Err(e) => eprintln!("失败: {e}"),
        }
        return;
    }

    let args: Vec<String> = std::env::args().collect();
    let as_version = args.iter().position(|a| a == "--as").and_then(|i| args.get(i + 1)).cloned();
    let resolve = args.iter().any(|a| a == "--resolve");

    let opts = PingOptions { use_srv: false, allow_legacy: false, timeout: t, ..Default::default() };
    let result = if let Some(v) = &as_version {
        println!("===== 现代 ping {addr} (以 {v} 协议) =====");
        minecraft::ping_as(&addr, v, &opts).await
    } else if resolve {
        println!("===== 现代 ping {addr} (两段式 resolve) =====");
        minecraft::ping_resolved(&addr, &opts).await
    } else {
        println!("===== 现代 ping {addr} =====");
        minecraft::ping_with(&addr, &opts).await
    };
    match result {
        Ok(r) => {
            let st = &r.status;
            println!("version    : {:?}  protocol {}", st.version.name, st.version.protocol);
            if let Some(p) = &st.players {
                println!("players    : {}/{}  sample={}", p.online, p.max, p.sample.len());
                for s in p.sample.iter().take(6) {
                    println!("             · {:?} {}", s.name, s.id);
                }
            }
            println!("latency    : {:?}", r.latency);
            println!(
                "secureChat : {:?}   previewsChat: {:?}   preventsChatReports: {:?}",
                st.enforces_secure_chat, st.previews_chat, st.prevents_chat_reports
            );
            println!(
                "favicon    : {}",
                st.favicon_png().map(|b| format!("{} bytes", b.len())).unwrap_or_else(|| "无".into())
            );
            println!(
                "forgeData  : {}   modinfo: {}   isModded: {:?}",
                st.forge_data.is_some(),
                st.modinfo.is_some(),
                st.is_modded
            );
            if let Some(mp) = &st.modpack {
                println!("modpack    : {:?} v{:?} (BCC betterStatus)", mp.name, mp.version);
            }
            if let Some(m) = st.mods() {
                println!(
                    "mods       : loader={:?} count={:?} channels={:?} truncated={}",
                    m.loader, m.mod_count, m.channel_count, m.truncated
                );
                for (id, ver) in m.mods.iter().take(12) {
                    println!("             · {id} {ver}");
                }
                if m.mods.len() > 12 {
                    println!("             · …(+{})", m.mods.len() - 12);
                }
            }
            println!("desc(plain): {:?}", st.description.plain());
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&r.raw_json) {
                let pretty = serde_json::to_string_pretty(&v).unwrap_or_default();
                // 截断 favicon 那种超长串,便于看结构
                let shown: String = pretty
                    .lines()
                    .map(|l| {
                        if l.len() > 160 {
                            format!("{}…(截断 {} 字符)", &l[..160], l.len() - 160)
                        } else {
                            l.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                println!("----- 原始 JSON -----\n{shown}");
            }
        }
        Err(e) => println!("现代失败: {e}"),
    }

    println!("\n===== 旧版 ping {addr} =====");
    let (h, p) = match addr.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(25565)),
        None => (addr.clone(), 25565),
    };
    match minecraft::legacy::ping_legacy(&h, p, t).await {
        Ok((ls, lat)) => {
            println!(
                "proto={} version={:?} online={}/{} latency={:?}",
                ls.protocol, ls.version, ls.online, ls.max, lat
            );
            println!("motd={:?}", ls.motd);
        }
        Err(e) => println!("旧版失败: {e}"),
    }

    println!("\n===== Query (UDP) {addr} =====");
    match minecraft::query::query(&h, p, t).await {
        Ok(q) => {
            println!("motd={:?}  version={:?}  map={:?}  gametype={:?}", q.motd, q.version, q.map, q.gametype);
            println!("players {}/{}: {:?}", q.online, q.max, q.players);
            println!("plugins={:?}", q.plugins);
            println!("hostip={}:{}  latency={:?}", q.host_ip, q.host_port, q.latency);
        }
        Err(e) => println!("Query 失败(多半 enable-query=false): {e}"),
    }
}
