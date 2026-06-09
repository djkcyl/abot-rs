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

进程配置经环境变量读取（没有文件型配置）；各项含义见 [`.env.example`](.env.example)。

## 网页控制台

进程内自带一个 axum 控制台（默认 `127.0.0.1:8080`，经 `WEB_BIND` 配）。网页点「获取验证码」、私聊机器人发 `登录 <验证码>` 即登录（复用 QQ 身份，主人/超管权限更高）。登录后可查看插件清单、处理待审、修改插件配置、浏览数据表。

前端在 [`web/`](web/)（Vue + Vite），需先构建（产物嵌进二进制；没构建时控制台只回提示页）：

```sh
cd web && pnpm install && pnpm build
```

## 数据层

`AUser` / `AGroup` 句柄（Model + 连接，无 DAO 层）+ 共享的「取或建」+ 等级曲线 + 个人面板贡献槽。各插件用 `nagisa::inventory` 自带建表迁移、由核心统一收集，互不引用。一切「每日重置」统一在**凌晨 4 点**刷新。

## License

MIT OR Apache-2.0
