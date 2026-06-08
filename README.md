# abot

基于 [**nagisa**](https://github.com/djkcyl/nagisa) 搭的多插件 QQ 机器人。

## 插件

一个目录一个插件，都在 [`src/plugins/`](src/plugins/)。

## 运行

前置：

- 一个 **OneBot v11 协议端**（如 [Lagrange.OneBot](https://github.com/LagrangeDev/Lagrange.Core)）登录 QQ、跑正向 WS 服务端；
- 一个 **PostgreSQL** 库（abot 用 `sqlx-postgres`，不支持 SQLite）。

步骤：

```sh
cp .env.example .env     # 改成你的 DATABASE_URL / ONEBOT_WS_URL / MASTER / SUPERUSERS
cargo run                # 启动即自动建表迁移，连上协议端
```

配置全部经环境变量读取，没有文件型配置；各项含义见 [`.env.example`](.env.example)。

## 数据层

`AUser` / `AGroup` 句柄（Model + 连接，无 DAO 层）+ 共享的「取或建」+ 等级曲线 + 个人面板贡献槽。各插件用 `nagisa::inventory` 自带建表迁移、由核心统一收集，互不引用。一切「每日重置」统一在**凌晨 4 点**刷新。

## License

MIT OR Apache-2.0
