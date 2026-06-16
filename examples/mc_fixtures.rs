//! 离线回归样本:把本会话各类服务端的**真机捕获**(`assets/minecraft/fixtures/*.json`,已去 favicon)
//! 与如实重构的状态 JSON 固化下来,解析后断言关键字段。改解析器后跑这个即可一键回归,无需再起服务器。
//!
//!   cargo run --example mc_fixtures

use abot::integrations::minecraft::StatusResponse;
use abot::integrations::minecraft::status::ModLoader;

fn parse(j: &str) -> StatusResponse {
    serde_json::from_str(j).expect("fixture 应解析成功")
}

fn main() {
    // ===================== 真机捕获 =====================

    // Forge 1.21.1 (FML3):forgeData 空 mods + 打包 d,15 位解码出真实模组列表
    {
        let st = parse(include_str!("../assets/minecraft/fixtures/forge_fml3.json"));
        let m = st.mods().expect("FML3 应识别为模组服");
        let ids: Vec<&str> = m.mods.iter().map(|(i, _)| i.as_str()).collect();
        println!("[forge_fml3]   proto={} d→mods={:?} count={:?}", st.version.protocol, ids, m.mod_count);
        assert_eq!(st.version.protocol, 767);
        assert_eq!(m.mod_count, Some(3), "real d 应解出 3 个模组");
        assert!(ids.contains(&"minecraft") && ids.contains(&"forge") && ids.contains(&"nochatreports"));
    }

    // Forge 1.12.2 (FML1):真实公网 RLCraft,modinfo 206 个模组
    {
        let st = parse(include_str!("../assets/minecraft/fixtures/forge_fml1_rlcraft.json"));
        let m = st.mods().expect("FML1 应识别");
        println!("[rlcraft_fml1] proto={} loader={:?} mod_count={:?}", st.version.protocol, m.loader, m.mod_count);
        assert_eq!(st.version.protocol, 340);
        assert_eq!(m.loader, ModLoader::Forge);
        assert_eq!(m.mod_count, Some(206), "RLCraft 应 206 个模组");
        assert!(m.mods.iter().any(|(i, _)| i == "minecraft"));
    }

    // CubeCraft:§码版本名 + 广告型 sample(6 条 §码名 + 全零 UUID)+ 空 modinfo stub
    {
        let st = parse(include_str!("../assets/minecraft/fixtures/sample_cubecraft.json"));
        let p = st.players.as_ref().expect("应有 players");
        println!("[cubecraft]    version={:?} sample={}", st.version.name, p.sample.len());
        assert_eq!(p.sample.len(), 6, "广告 sample 应全部保留、不报错");
        assert!(p.sample[0].name.contains("CubeCraft"));
        assert!(p.sample.iter().all(|s| s.id == "00000000-0000-0000-0000-000000000000"), "全零 UUID 应原样保留");
        assert_eq!(st.mods().and_then(|m| m.mod_count), Some(0), "空 modinfo stub 计数 0(展示层据此不显示)");
    }

    // ===================== 如实重构(本会话真机观察到的结构)=====================

    // Vanilla 26.1.2:裸串 MOTD + 强制安全聊天 true(真机验过)
    {
        let st = parse(r#"{"version":{"name":"26.1.2","protocol":775},"players":{"max":20,"online":0},"description":"§dVanilla","enforcesSecureChat":true}"#);
        println!("[vanilla]      proto={} secureChat={:?}", st.version.protocol, st.enforces_secure_chat);
        assert_eq!(st.version.protocol, 775);
        assert_eq!(st.enforces_secure_chat, Some(true));
        assert!(st.mods().is_none());
    }

    // NeoForge:仅顶层 isModded(无模组明细)
    {
        let st = parse(r#"{"version":{"name":"1.21.1","protocol":767},"isModded":true,"description":"NeoForge"}"#);
        let m = st.mods().expect("isModded 应识别为带模组");
        println!("[neoforge]     loader={:?} count={:?}", m.loader, m.mod_count);
        assert_eq!(m.loader, ModLoader::Modded);
        assert_eq!(m.mod_count, None, "NeoForge 给不了明细");
    }

    // BCC betterStatus:整合包名 + 版本(现代版 camelCase key)
    {
        let st = parse(r#"{"betterStatus":{"name":"All The Mods 10","version":"4.4"},"isModded":true,"version":{"protocol":767}}"#);
        let mp = st.modpack.as_ref().expect("应解析 betterStatus");
        println!("[bcc]          modpack={:?} v{:?}", mp.name, mp.version);
        assert_eq!((mp.name.as_str(), mp.version.as_str()), ("All The Mods 10", "4.4"));
    }

    // BCC 旧版 Forge 1.20.1:连字符 key `better-status`;未配置时默认值 "??" 应归一为空
    {
        let st = parse(r#"{"better-status":{"name":"RAD2","version":"1.20"},"version":{"protocol":763}}"#);
        let mp = st.modpack.as_ref().expect("连字符 better-status 应解析");
        println!("[bcc-hyphen]   modpack={:?} v{:?}", mp.name, mp.version);
        assert_eq!((mp.name.as_str(), mp.version.as_str()), ("RAD2", "1.20"));
        assert!(parse(r#"{"betterStatus":{"name":"??","version":"??"}}"#).modpack.is_none(), "占位 ?? 应忽略");
    }

    // ViaVersion:非标准 version.supportedVersions(send-supported-versions:true 时注入)
    {
        let st = parse(r#"{"version":{"name":"Paper 1.21","protocol":767,"supportedVersions":[47,340,754,767,775]}}"#);
        println!("[via]          protocol={} supported={:?}", st.version.protocol, st.version.supported_versions);
        assert_eq!(st.version.supported_versions, vec![47, 340, 754, 767, 775], "应解析 Via supportedVersions 数组");
        // 普通服务器无此字段 → 空
        assert!(parse(r#"{"version":{"protocol":767}}"#).version.supported_versions.is_empty(), "无该字段应为空");
        // 字段类型乱报(非数组)也不应让整体失败
        assert!(parse(r#"{"version":{"protocol":767,"supportedVersions":"oops"}}"#).version.supported_versions.is_empty(), "非数组应降级为空");
    }

    // 版本名 → 协议号映射(按版本 ping 用)
    {
        use abot::integrations::minecraft::versions::protocol_for;
        assert_eq!(protocol_for("1.8"), Some(47));
        assert_eq!(protocol_for("1.12.2"), Some(340));
        assert_eq!(protocol_for("1.16"), Some(754));
        assert_eq!(protocol_for("26.1"), Some(775));
        assert_eq!(protocol_for("47"), Some(47), "直接给协议号也认");
        assert_eq!(protocol_for("不存在"), None);
        println!("[versions]     1.8→47 1.12.2→340 1.16→754 26.1→775 ✔");
    }

    // Forge FML2:明文 mods 数组 + channels(1.13–1.17 形态)
    {
        let st = parse(r#"{"version":{"protocol":754},"forgeData":{"fmlNetworkVersion":2,"mods":[{"modId":"forge","modmarker":"ANY"},{"modId":"jei","modmarker":"9.7"}],"channels":[{"res":"jei:channel","version":"1","required":false}]}}"#);
        let m = st.mods().expect("FML2");
        println!("[forge_fml2]   count={:?} channels={:?}", m.mod_count, m.channel_count);
        assert_eq!(m.mod_count, Some(2));
        assert_eq!(m.channel_count, Some(1));
    }

    // Velocity 代理:版本名伪装 + 无 modinfo/forgeData
    {
        let st = parse(r#"{"version":{"name":"Velocity 1.7.2-1.21.11","protocol":774},"players":{"max":500,"online":0},"description":"A Velocity Server"}"#);
        println!("[velocity]     version={:?} mods={:?}", st.version.name, st.mods().is_some());
        assert!(st.version.name.as_deref().unwrap_or("").starts_with("Velocity"));
        assert!(st.mods().is_none(), "纯代理无模组信息");
    }

    // No Chat Reports:顶层 preventsChatReports
    {
        let st = parse(r#"{"description":"x","preventsChatReports":true}"#);
        assert_eq!(st.prevents_chat_reports, Some(true));
    }

    println!("\n所有回归样本通过 ✔");
}
