//! 离线自测:本地起一个「假状态服务器」,用本模块的 ping 客户端连它,验证编解码 / 握手 /
//! 状态解析 / 测延迟全链路,再把 MOTD 渲染成 PNG。**无需联网**。
//!
//!   cargo run --example mc_selftest
//!
//! 产物落到临时目录(路径会打印)。

use std::time::Duration;

use abot::integrations::minecraft::{self, PingOptions, codec};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const STATUS_JSON: &str = r##"{
  "version": { "name": "Paper 1.20.4", "protocol": 765 },
  "players": { "max": 100, "online": 42, "sample": [
    { "name": "§aAlice", "id": "00000000-0000-0000-0000-000000000001" },
    { "name": "Bob", "id": "00000000-0000-0000-0000-000000000002" }
  ]},
  "description": { "text": "", "extra": [
    { "text": "ABot ", "color": "#55FFFF", "bold": true },
    { "text": "Minecraft 服务器\n", "color": "gold" },
    { "text": "欢迎 ", "color": "green" },
    { "text": "§l§d测试§r §7| ", "color": "white" },
    { "text": "在线中", "color": "#FF8800", "underlined": true }
  ]},
  "favicon": "FAVICON_PLACEHOLDER",
  "enforcesSecureChat": false
}"##;

/// 造一个带透明背景的 64×64 favicon(蓝圆 + 透明四角),验证镂空处叠棋盘格。
fn make_favicon() -> String {
    use base64::Engine as _;
    use image::{ImageEncoder, Rgba, RgbaImage};
    let mut im = RgbaImage::new(64, 64);
    for y in 0..64u32 {
        for x in 0..64u32 {
            let (dx, dy) = (x as f32 - 31.5, y as f32 - 31.5);
            if (dx * dx + dy * dy).sqrt() < 28.0 {
                im.put_pixel(x, y, Rgba([80, 180, 250, 255])); // 不透明蓝圆
            } // 圆外保持透明
        }
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(im.as_raw(), 64, 64, image::ExtendedColorType::Rgba8)
        .unwrap();
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png))
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind 失败");
    let port = listener.local_addr().unwrap().port();
    let json = STATUS_JSON.replace("FAVICON_PLACEHOLDER", &make_favicon());
    tokio::spawn(async move {
        if let Ok((sock, _)) = listener.accept().await {
            serve(sock, json, None).await;
        }
    });

    let target = format!("127.0.0.1:{port}");
    let opts = PingOptions { use_srv: false, timeout: Duration::from_secs(3), ..Default::default() };
    let result = minecraft::ping_with(&target, &opts).await.expect("ping 失败");

    let st = &result.status;
    println!("== 状态 ==");
    println!("  version : {:?} (协议 {})", st.version.name, st.version.protocol);
    if let Some(p) = &st.players {
        println!("  players : {}/{} (抽样 {})", p.online, p.max, p.sample.len());
    }
    println!("  延迟    : {:?}", result.latency);
    println!("  安全聊天: {:?}", st.enforces_secure_chat);

    let spans = st.description.to_spans();
    println!("== MOTD 纯文本 ==\n{}", st.description.plain());
    println!("== MOTD ANSI ==\n{}", minecraft::component::to_ansi(&spans));
    println!("== span 序列({} 段)==", spans.len());
    for sp in &spans {
        println!("  {:?}  {:?}", sp.style.color, sp.text);
    }

    // 断言关键事实,自测真出错就 panic
    assert_eq!(st.version.protocol, 765);
    assert_eq!(st.players.as_ref().unwrap().online, 42);
    assert!(result.latency.is_some(), "应测到延迟");
    assert!(spans.iter().any(|s| matches!(s.style.color, Some(minecraft::color::Color::Rgb(..)))), "应解析出 hex 色");
    assert!(spans.iter().any(|s| s.style.bold), "应解析出加粗");
    assert!(spans.iter().any(|s| s.text.contains('服')), "应含 CJK");

    // 合成校验未被实测覆盖的解析支路:FML1(modinfo)、FML2(明文 mods)、宽松整数(字符串数字)
    println!("\n== 合成解析校验 ==");
    let cases = [
        (
            "FML1",
            r#"{"version":{"name":"1.12.2","protocol":340},"modinfo":{"type":"FML","modList":[{"modid":"forge","version":"14.23.5"},{"modid":"jei","version":"4.16"}]}}"#,
        ),
        (
            "FML2",
            r#"{"version":{"name":"1.16.5","protocol":754},"forgeData":{"fmlNetworkVersion":2,"mods":[{"modId":"forge","modmarker":"ANY"},{"modId":"ironchest","modmarker":"1.16.5-11"}],"channels":[{"res":"ic:main","version":"1","required":false}]}}"#,
        ),
        ("宽松整数", r#"{"version":{"name":"x","protocol":"765"},"players":{"max":"100","online":"7.0"}}"#),
        (
            "坏sample+translate",
            r#"{"players":{"max":20,"online":3,"sample":{"oops":1}},"description":{"translate":"x","fallback":"回退文案"}}"#,
        ),
        ("players非对象", r#"{"players":"nope","version":{"protocol":"767"}}"#),
    ];
    for (label, json) in cases {
        let st: minecraft::StatusResponse = serde_json::from_str(json).expect("解析失败");
        let mods = st.mods();
        println!(
            "  [{label}] protocol={} players={:?} mods={:?}",
            st.version.protocol,
            st.players.as_ref().map(|p| (p.online, p.max)),
            mods.as_ref().map(|m| (m.loader, m.mod_count)),
        );
        if label == "FML1" {
            assert_eq!(mods.as_ref().unwrap().mod_count, Some(2), "FML1 应解出 2 个 mod");
        }
        if label == "FML2" {
            assert_eq!(mods.as_ref().unwrap().mod_count, Some(2), "FML2 应解出 2 个 mod");
            assert_eq!(mods.as_ref().unwrap().channel_count, Some(1));
        }
        if label == "宽松整数" {
            assert_eq!(st.version.protocol, 765, "字符串协议号应解析");
            let p = st.players.as_ref().unwrap();
            assert_eq!((p.online, p.max), (7, 100), "字符串/浮点人数应解析");
        }
        if label == "坏sample+translate" {
            let p = st.players.as_ref().expect("坏 sample 不应让整体解析失败");
            assert_eq!((p.online, p.max), (3, 20));
            assert!(p.sample.is_empty(), "对象型 sample 应降级为空,不报错");
            assert_eq!(st.description.plain(), "回退文案", "translate 节点应取 fallback");
        }
        if label == "players非对象" {
            assert!(st.players.is_none(), "非对象 players 应为 None,不报错");
            assert_eq!(st.version.protocol, 767);
        }
    }

    // 根本形态矩阵:涵盖空 JSON / 缺字段 / 各 MOTD 形态 / favicon 变体 / 整体非对象 —— 只要是合法
    // JSON 就绝不 panic、绝不整体失败,坏字段降级。
    println!("\n== 根本形态矩阵(永不 panic / 坏字段降级)==");
    const TINY: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    let shapes: Vec<(&str, String)> = vec![
        ("空对象", "{}".into()),
        ("缺description", r#"{"version":{"name":"1.21","protocol":767}}"#.into()),
        ("description数组", r#"{"description":["第一段 ","§e第二段"]}"#.into()),
        ("description裸串§", r#"{"description":"§aHello §c世界"}"#.into()),
        ("version缺name", r#"{"version":{"protocol":767}}"#.into()),
        ("缺version", r#"{"description":"x"}"#.into()),
        ("secureChat字符串", r#"{"enforcesSecureChat":"true"}"#.into()),
        ("负数人数", r#"{"players":{"max":-1,"online":-1}}"#.into()),
        ("favicon无前缀", format!(r#"{{"favicon":"{TINY}"}}"#)),
        ("favicon带空白MIME", format!(r#"{{"favicon":" data:image/jpeg;base64, {TINY} "}}"#)),
        ("整体非对象", r#""just a string""#.into()),
        ("整体数组", "[1,2,3]".into()),
    ];
    for (label, json) in &shapes {
        let st: minecraft::StatusResponse = serde_json::from_str(json).expect("合法 JSON 不应解析失败");
        println!(
            "  [{label:<14}] proto={} name={:?} players={:?} desc={:?} favicon={}",
            st.version.protocol,
            st.version.name,
            st.players.as_ref().map(|p| (p.online, p.max)),
            st.description.plain(),
            st.favicon_png().map(|b| format!("{}B", b.len())).unwrap_or_else(|| "无".into()),
        );
    }
    // 关键断言
    let parse = |j: &str| serde_json::from_str::<minecraft::StatusResponse>(j).unwrap();
    assert_eq!(parse("{}").version.protocol, 0);
    assert_eq!(parse(r#"{"description":["A","B"]}"#).description.plain(), "AB");
    assert!(parse(r#"{"version":{"protocol":767}}"#).version.name.is_none());
    assert_eq!(parse(r#"{"enforcesSecureChat":"true"}"#).enforces_secure_chat, Some(true));
    let neg = parse(r#"{"players":{"max":-1,"online":-1}}"#).players.unwrap();
    assert_eq!((neg.online, neg.max), (-1, -1), "负数人数不应被夹");
    assert!(parse(&format!(r#"{{"favicon":"{TINY}"}}"#)).favicon_png().is_some(), "无前缀 favicon 应解出");
    assert!(
        parse(&format!(r#"{{"favicon":" data:image/jpeg;base64, {TINY} "}}"#)).favicon_png().is_some(),
        "带空白/异 MIME favicon 应解出"
    );
    assert!(parse(r#""x""#).players.is_none(), "整体非对象应降级为默认");
    assert_eq!(
        parse(r#"{"description":{"translate":"%s 加入 %s","with":["A","世界"]}}"#).description.plain(),
        "A 加入 世界",
        "translate %s 应被 with 顺序填充"
    );
    assert_eq!(
        parse(r#"{"description":{"translate":"%2$s-%1$s","with":["a","b"]}}"#).description.plain(),
        "b-a",
        "translate %N$s 应按位填充"
    );
    let mp = parse(r#"{"betterStatus":{"name":"ATM10","version":"1.5.0"}}"#)
        .modpack
        .expect("BCC betterStatus 应解析为整合包");
    assert_eq!((mp.name.as_str(), mp.version.as_str()), ("ATM10", "1.5.0"), "整合包名+版本应提取");
    assert!(parse(r#"{"betterStatus":{"name":"","version":""}}"#).modpack.is_none(), "空 betterStatus 应忽略");
    // BCC 旧版 Forge 1.20.1 用连字符 key `better-status`
    let hy =
        parse(r#"{"better-status":{"name":"RAD2","version":"1.20"}}"#).modpack.expect("连字符 better-status 应解析");
    assert_eq!((hy.name.as_str(), hy.version.as_str()), ("RAD2", "1.20"), "连字符 key 同样提名+版本");
    // BCC 未配置时默认值是 "??",应归一为空而非当真实值
    assert!(parse(r#"{"betterStatus":{"name":"??","version":"??"}}"#).modpack.is_none(), "占位 ?? 应被归一忽略");

    // Nyf's Modpack Version Check:版本不在状态 JSON 里,而是追加在 pong 尾部。起一个会追加尾部的
    // 假服务器,走完整 ping/pong 链路,验证客户端能从 pong 取出版本并补进 modpack。
    {
        let nyf_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind 失败");
        let nyf_port = nyf_listener.local_addr().unwrap().port();
        let nyf_json = r#"{"version":{"name":"1.20.1","protocol":763},"players":{"max":20,"online":1}}"#.to_string();
        tokio::spawn(async move {
            if let Ok((sock, _)) = nyf_listener.accept().await {
                serve(sock, nyf_json, Some("MyPack-3.2.1".into())).await;
            }
        });
        let nyf_res = minecraft::ping_with(
            &format!("127.0.0.1:{nyf_port}"),
            &PingOptions { use_srv: false, timeout: Duration::from_secs(3), ..Default::default() },
        )
        .await
        .expect("Nyf's ping 失败");
        let mp = nyf_res.status.modpack.as_ref().expect("应从 pong 尾部取出整合包版本");
        assert_eq!(mp.version, "MyPack-3.2.1", "Nyf's 整合包版本应来自 pong 尾部");
        assert!(mp.name.is_empty(), "Nyf's 只给版本,无整合包名");
        println!("[nyf]  pong 尾部整合包版本 = {:?}", mp.version);
    }

    test_additional_edges();
    test_bidi();
    test_via_proxy_resolve().await;
    test_bedrock().await;

    let dir = std::env::temp_dir();
    let motd = minecraft::render::render_motd_png(&spans, &Default::default()).unwrap();
    let motd_path = dir.join("mc_selftest_motd.png");
    std::fs::write(&motd_path, &motd).unwrap();

    // 1.8.9 目标:hex 色识别不了,按策略降到最近命名色
    let old_opts =
        minecraft::render::RenderOptions { target: minecraft::render::TargetVersion::V1_8_9, ..Default::default() };
    let motd_old = minecraft::render::render_motd_png(&spans, &old_opts).unwrap();
    let old_path = dir.join("mc_selftest_motd_1.8.9.png");
    std::fs::write(&old_path, &motd_old).unwrap();

    let card = minecraft::render::render_server_card_png(&result, &Default::default()).unwrap();
    let card_path = dir.join("mc_selftest_card.png");
    std::fs::write(&card_path, &card).unwrap();

    println!("\n== 产物 ==");
    println!("  MOTD       : {} ({} 字节)", motd_path.display(), motd.len());
    println!("  MOTD 1.8.9 : {} ({} 字节)", old_path.display(), motd_old.len());
    println!("  整卡       : {} ({} 字节)", card_path.display(), card.len());
    println!("\n全链路自测通过 ✔");
}

/// 验证 `ping_resolved` 穿透 ViaProxy:假代理只「认」一个老版本集合(故意**不含**我们的最新版 775),
/// 对 -1 / 未注册协议回 ViaProxy 占位,对认得的协议回后端真实 status。resolved 应靠阶梯命中老版本拿到后端,
/// 而不是无脑用 775(那样会被占位挡回)。
async fn test_via_proxy_resolve() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let real =
        r#"{"version":{"name":"Paper 1.20.1","protocol":763},"players":{"max":20,"online":3},"description":"Backend"}"#;
    // 代理只支持 763(1.20.1)这一档(阶梯里有、但不是最新);775 会被当未注册退占位。
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(serve_proxy(sock, real.to_string(), vec![763]));
        }
    });
    let target = format!("127.0.0.1:{port}");
    let opts = PingOptions { use_srv: false, timeout: Duration::from_secs(3), ..Default::default() };

    let placeholder = minecraft::ping_with(&target, &opts).await.expect("ping");
    assert_eq!(placeholder.status.version.name.as_deref(), Some("ViaProxy"), "默认 -1 应拿到 ViaProxy 占位");

    let resolved = minecraft::ping_resolved(&target, &opts).await.expect("resolved");
    assert_eq!(
        resolved.status.version.name.as_deref(),
        Some("Paper 1.20.1"),
        "resolved 应阶梯命中老版本穿透后端,而非被 775 挡回"
    );
    assert_eq!(resolved.status.players.as_ref().unwrap().online, 3);
    println!("[viaproxy] -1→占位;resolved 走阶梯命中 1.20.1(763)穿透后端 ✔(未硬用最新版 775)");
}

/// 假 ViaProxy:按握手里的协议号决定回占位还是后端真实 status。
async fn serve_proxy(mut s: TcpStream, real_json: String, supported: Vec<i32>) {
    let hs = read_frame(&mut s).await.unwrap_or_default();
    let mut r = codec::Reader::new(&hs);
    let _ = r.read_varint(); // 包 ID 0x00
    let proto = r.read_varint().unwrap_or(-1); // 握手协议号
    let _ = read_frame(&mut s).await; // status request
    let json = if supported.contains(&proto) {
        real_json
    } else {
        r#"{"version":{"name":"ViaProxy","protocol":-1},"players":{"max":0,"online":0},"description":"Your client version is not supported by ViaProxy!"}"#.to_string()
    };
    let mut payload = Vec::new();
    codec::write_string(&mut payload, &json);
    let _ = s.write_all(&codec::packet(0x00, &payload)).await;
    let _ = s.flush().await;
    if let Ok(frame) = read_frame(&mut s).await {
        let mut rr = codec::Reader::new(&frame);
        let _ = rr.read_varint();
        if let Ok(echo) = rr.read_i64() {
            let mut p = Vec::new();
            codec::write_i64(&mut p, echo);
            let _ = s.write_all(&codec::packet(0x01, &p)).await;
            let _ = s.flush().await;
        }
    }
}

/// 假状态服务器:读握手 + status request,回 status response,再读 ping、回 pong。
/// `nyf` 非空时,模拟 Nyf's Modpack Version Check 在 pong 标准载荷后追加整合包版本 + 服务器 IP。
async fn serve(mut s: TcpStream, json: String, nyf: Option<String>) {
    let _ = read_frame(&mut s).await; // handshake
    let _ = read_frame(&mut s).await; // status request

    let mut payload = Vec::new();
    codec::write_string(&mut payload, &json);
    let _ = s.write_all(&codec::packet(0x00, &payload)).await;
    let _ = s.flush().await;

    if let Ok(frame) = read_frame(&mut s).await {
        let mut r = codec::Reader::new(&frame);
        let _ = r.read_varint(); // 包 ID 0x01
        if let Ok(echo) = r.read_i64() {
            let mut p = Vec::new();
            codec::write_i64(&mut p, echo);
            if let Some(ver) = &nyf {
                codec::write_string(&mut p, ver); // 整合包版本
                codec::write_string(&mut p, "play.example.net"); // 服务器 IP
            }
            let _ = s.write_all(&codec::packet(0x01, &p)).await;
            let _ = s.flush().await;
        }
    }
}

async fn read_frame(s: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut len: i32 = 0;
    let mut shift = 0;
    loop {
        let mut b = [0u8; 1];
        s.read_exact(&mut b).await?;
        len |= ((b[0] & 0x7F) as i32) << shift;
        if b[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 35 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "VarInt 过长"));
        }
    }
    let mut buf = vec![0u8; len.max(0) as usize];
    s.read_exact(&mut buf).await?;
    Ok(buf)
}

// ADDED EDGE CASE TESTS - DO NOT COMMIT
/// Bedrock(RakNet/UDP)ping:本地起一个假基岩服(应答 Unconnected Pong),验证发包/解析/映射/渲染全链路。
async fn test_bedrock() {
    use abot::integrations::minecraft::bedrock::{self, BedrockOptions};
    const MAGIC: [u8; 16] =
        [0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78];
    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("udp bind");
    let port = sock.local_addr().unwrap().port();
    // 含 § 码 + CJK 的 MOTD,12 段(官方 BDS 形态)
    let motd = "MCPE;§l本地基岩测试服;800;1.21.90;7;50;1234605616436508552;Bedrock level;Survival;1;19132;19133;";
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        while let Ok((n, peer)) = sock.recv_from(&mut buf).await {
            if n < 33 || buf[0] != 0x01 {
                continue; // 只认 Unconnected Ping
            }
            let mut out = Vec::with_capacity(35 + motd.len());
            out.push(0x1c); // Unconnected Pong
            out.extend_from_slice(&buf[1..9]); // 回显 ping_time
            out.extend_from_slice(&0x1122_3344_5566_7788u64.to_be_bytes()); // server GUID
            out.extend_from_slice(&MAGIC);
            out.extend_from_slice(&(motd.len() as u16).to_be_bytes());
            out.extend_from_slice(motd.as_bytes());
            let _ = sock.send_to(&out, peer).await;
        }
    });

    let r = bedrock::ping_bedrock(
        &format!("127.0.0.1:{port}"),
        &BedrockOptions { timeout: Duration::from_secs(2), attempts: 3 },
    )
    .await
    .expect("bedrock ping");
    let s = &r.status;
    assert_eq!(s.edition, "MCPE");
    assert_eq!(s.protocol, 800);
    assert_eq!(s.version_name, "1.21.90");
    assert_eq!((s.online, s.max), (7, 50), "在线/最大人数");
    assert_eq!(s.motd_line1, "§l本地基岩测试服");
    assert_eq!(s.motd_line2, "Bedrock level");
    assert_eq!(s.gamemode, "Survival");
    assert_eq!(s.port_v4, Some(19132));
    assert_eq!(s.port_v6, Some(19133));
    // 映射成统一 PingResult + 整卡渲染不 panic
    let pr = bedrock::to_ping_result(&r);
    assert_eq!(pr.status.players.as_ref().unwrap().online, 7);
    assert!(pr.status.version.name.as_deref().unwrap().starts_with("Bedrock"));
    let _ = minecraft::render::render_server_card_png(&pr, &Default::default()).unwrap();
    println!("[bedrock] 本地假基岩服:MCPE 1.21.90 · 7/50 · Survival,字段解析+映射+渲染 ✔");
}

/// bidi/RTL:复刻原版 ClientLanguage.getVisualOrder —— 逐行先阿拉伯塑形再 bidi 重排,可见序应与原版一致。
fn test_bidi() {
    use minecraft::Component;
    use minecraft::render::visual_lines;
    let big = 100_000u32;
    let vis = |s: &str| visual_lines(&Component::text(s).to_spans(), big, 4, 1).join("\n");

    // 纯拉丁:顺序不变
    assert_eq!(vis("Hello 世界"), "Hello 世界", "纯 LTR(含 CJK)不应重排");

    // 纯希伯来:整行反转为可见序(无塑形)。logical שלום → visual מולש
    let heb = vis("שלום");
    assert_eq!(heb.chars().rev().collect::<String>(), "שלום", "希伯来应整体反转");

    // 拉丁+阿拉伯+拉丁:整体 LTR 框架保留,中间阿拉伯段反转 + 塑形成 presentation form
    let mix = vis("AB مرحبا CD");
    assert!(mix.starts_with("AB"), "LTR 前缀在最前: {mix}");
    assert!(mix.ends_with("CD"), "LTR 后缀在最后: {mix}");
    assert!(
        mix.chars().any(|c| matches!(c as u32, 0xFE70..=0xFEFF | 0xFB50..=0xFDFF)),
        "阿拉伯字母应被塑形成 presentation form: {mix}"
    );
    // 塑形改变了码点(连写),可见序不应等于朴素的「逻辑序反转中段」
    assert_ne!(mix, "AB مرحبا CD", "应发生重排/塑形");

    println!("[bidi] 拉丁不动 · 希伯来整体反转 · 阿-拉混排 LTR 框架内反转+塑形 ✔");
}

/// 审计指出但此前没断言的「合法但刁钻」字段形态:都应安静降级,绝不 panic。每条都核对最终取值。
fn test_additional_edges() {
    let parse = |j: &str| serde_json::from_str::<minecraft::StatusResponse>(j).unwrap();

    // color 是数字 → 非字符串,as_str() 为 None → 无色(用渲染默认色)
    let c1 = parse(r#"{"description":{"text":"x","color":123}}"#).description.to_spans();
    assert_eq!(c1.first().and_then(|s| s.style.color), None, "数字型 color 应当无色降级");

    // color 是非法 hex(含非十六进制位)→ 无色
    let c2 = parse(r##"{"description":{"text":"x","color":"#GGGGGG"}}"##).description.to_spans();
    assert_eq!(c2.first().and_then(|s| s.style.color), None, "非法 hex color 应当无色降级");

    // version.protocol 是对象 → 非数字/字符串 → 取 0
    assert_eq!(parse(r#"{"version":{"protocol":{}}}"#).version.protocol, 0, "对象型 protocol 应降为 0");

    // players.max 是布尔 → as_i64_lenient 不接布尔 → 取 0(online 正常)
    let p6 = parse(r#"{"players":{"max":true,"online":5}}"#).players.unwrap();
    assert_eq!((p6.max, p6.online), (0, 5), "布尔型 max 应降为 0,不影响 online");

    // description 数组首元素是 null → 根为空、其余进 extra
    assert_eq!(parse(r#"{"description":[null,"text"]}"#).description.plain(), "text", "数组含 null 应跳过、保留其余");

    // translate %N$s 越界(只有 2 个参数却引用 %5$s)→ 该占位符留空,不 panic
    assert_eq!(
        parse(r#"{"description":{"translate":"%5$s","with":["a","b"]}}"#).description.plain(),
        "",
        "越界 %N$s 应留空"
    );

    println!("  附加边角形态:数字/非法 color、对象 protocol、布尔 max、含 null 数组、越界 translate —— 全部安静降级 ✔");
}
