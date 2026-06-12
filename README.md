# ABot

基于 [**nagisa**](https://github.com/djkcyl/nagisa) 搭的多插件 QQ 机器人。

## 运行

前置：

- 一个 **OneBot v11 协议端**（如 [Lagrange.OneBot](https://github.com/LagrangeDev/Lagrange.Core)）登录 QQ、跑正向 WS 服务端；
- 一个 **PostgreSQL** 库。

```sh
cp .env.example .env     # 填 DATABASE_URL / ONEBOT_WS_URL / MASTER 等
cargo run                # 启动自动建表迁移，连上协议端
```

配置只走环境变量，各项含义见 [`.env.example`](.env.example)。

## 网页控制台

进程内自带，默认 `127.0.0.1:8800`（经 `WEB_BIND` 配）。网页取验证码、私聊机器人发验证码（或 `登录 <验证码>`）即登录（复用 QQ 身份）。前端在 [`web/`](web/)，产物嵌进二进制，没构建时只回提示页：

```sh
pnpm -C web install && pnpm -C web build
```

## 代码

一个目录一个插件，都在 [`src/plugins/`](src/plugins/)；插件自带建表迁移，由核心统一收集。无测试，门禁是 `cargo build` + `cargo clippy --all-targets -- -D warnings` + `cargo doc --no-deps` 零警告。

## License

MIT OR Apache-2.0
