//! ping 一批知名 MC 服务器,把成功响应的前 10 个各渲染成列表卡,竖排合成一张长图。
//!
//!   cargo run --example mc_gallery                 # 用内置候选名单
//!   cargo run --example mc_gallery -- a.com b:25565 ...   # 自定义地址
//!
//! 产物: /tmp/mc_gallery.png

use std::time::Duration;

use abot::integrations::minecraft::render::{CardOptions, stack_vertical};
use abot::integrations::minecraft::{self, PingOptions};
use futures::future::join_all;

/// 候选服务器(给得多一些、覆盖更多类型/地区,挑前 WANT 个能 ping 通的)。
const CANDIDATES: &[&str] = &[
    // 大型网络
    "mc.hypixel.net",
    "play.cubecraft.net",
    "mc.mineplex.com",
    "play.gommehd.net",
    "play.pika-network.net",
    "hub.opblocks.com",
    "play.vortexnetwork.net",
    // 无政府 / 技术
    "2b2t.org",
    "constantiam.net",
    // 生存 / 城镇
    "play.earthmc.net",
    "mc.advancius.net",
    "mp.minecraftonline.com",
    // 玩法 / 小游戏 / prison / skyblock
    "play.manacube.com",
    "play.purpleprison.org",
    "play.wildprison.net",
    "play.applemc.fun",
    "play.invadedlands.net",
    "play.minehut.com",
    "play.cosmicprison.com",
    // 代理 / 跨平台(Geyser 等)
];

const WANT: usize = 13;

#[tokio::main]
async fn main() {
    let custom: Vec<String> = std::env::args().skip(1).collect();
    let targets: Vec<String> =
        if custom.is_empty() { CANDIDATES.iter().map(|s| s.to_string()).collect() } else { custom };

    let opts = PingOptions { timeout: Duration::from_secs(6), ..Default::default() };

    // 全部并发 ping
    let futs = targets.into_iter().map(|addr| {
        let opts = opts.clone();
        async move {
            let res = minecraft::ping_with(&addr, &opts).await;
            (addr, res)
        }
    });
    let results = join_all(futs).await;

    // 按候选顺序收集成功的,渲染列表卡,取前 WANT 个
    let mut cards = Vec::new();
    for (addr, res) in results {
        match res {
            Ok(r) => {
                let ms = r.latency.map(|d| d.as_millis()).unwrap_or(0);
                let pc = r
                    .status
                    .players
                    .as_ref()
                    .map(|p| format!("{}/{}", p.online, p.max))
                    .unwrap_or_default();
                println!("✔ {addr:<26} {ms:>4}ms  {pc}");
                let opts = CardOptions { title: Some(addr.clone()), ..Default::default() };
                cards.push(minecraft::render::render_server_card(&r, &opts));
                if cards.len() >= WANT {
                    break;
                }
            }
            Err(e) => println!("�’ {addr:<26} 失败: {e}"),
        }
    }

    if cards.is_empty() {
        eprintln!("一个都没 ping 通");
        std::process::exit(1);
    }

    let long = stack_vertical(&cards, 12, [12, 12, 14, 255]);
    let png = minecraft::render::encode_png(&long).unwrap();
    let path = std::env::temp_dir().join("mc_gallery.png");
    std::fs::write(&path, &png).unwrap();

    println!(
        "\n长图: {}  ({}x{}, {} 张卡)",
        path.display(),
        long.width(),
        long.height(),
        cards.len()
    );
}
