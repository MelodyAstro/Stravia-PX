# 请求模型映射部署与清理记录

2026-11-07（Asia/Pyongyang）已原生部署，仍基于官方 v0.6.4
`a100ef42031073fbdd779e4809fb83cac3a8b956`。运行代码对应
`dd62175e0d3983ffe71770c58770d866eb0649a4`；后续提交补充运维工具和本记录。

## 当前行为

- GPT-Private 与 All 已各启用 8 条短名称映射，客户端可发送 `gpt-6-luna` 等名称。
  原 Key 和 Base URL 保持不变：OpenAI `/comments`，Anthropic `/reviews`。
- GPT Responses WS 主入口：`wss://masub-ex.retlinks.top/comments/responses`；
  Cloudflare：`wss://masub-ex.yuhatze.org/comments/responses`。使用原 Key 和短模型名，
  两入口已完成实际 WebSocket 响应及工具往返验证。
- API Keys → 已有 Key → 请求模型映射，可设置开关、自定义名称和授权目标。
  三分钟识别记录模型、协议、传输方式和数量，并提供候选；保存后才生效。
  识别不保存聊天正文、headers、密钥或工具 Schema，结果十五分钟过期。
- 映射按 Key 隔离，目标为稳定路由 ID；缺失或撤权不会改用其它账号。
  响应使用客户端名称，内部上游模型、计费及续接身份保持原值。
- 原六个 Key、十一个路由及 Nginx 配置指纹保持一致。Team 和官方 Claude 空池继续禁用。
  GA Claude Code 开关仍开启，Debug 关闭。没有使用 Docker。

## 验证

- 9 项 Linux 原生回归全部通过：同名分组隔离、短名称、自定义名称、流式响应、
  权限撤回、重启持久化、识别边界及内部/客户端模型名称分离。
- 575 项网页单元测试和 11 项浏览器回归通过；覆盖 320/500/1280 px，包含中文暗色界面。
  Svelte 检查 0 错误/0 警告，修改文件的 ESLint 通过。
- 9 项隔离升级、3 项恢复、1 项快照测试通过，缺少活动功能标记的升级候选在切换前被拒绝。
- 两域名以短名称完成 HTTP、WebSocket、非默认 `px_diag.echo` 命名空间及工具结果往返，
  共八个完整响应；额外自定义 `px-luna` 请求也完整返回且保留别名。
- 识别实际捕获 HTTP/WS 样式并唯一预选授权目标；重复映射、跨组目标写入及跨组调用被拒绝。
  六组模型列表、普通 Key 禁止管理映射、401/403/404 边界均通过。临时别名已撤销，识别已清空。
- Claude Sonnet、Opus 的合成 CC 请求（7 消息、38 工具、32000 上限）及字典工具往返均通过。
- 两域名网页与静态资源 200，HTML no-store、哈希资源 immutable。Cloudflare 对 Python 默认
  User-Agent 的首次页面探测返回 403；浏览器 User-Agent 验证通过，入口规则未调整。
- 当前完整快照创建后实际重启，映射配置保持一致，Private 仍展示八个短名称，四个组件 ready。
  SQLite 完整性及外键检查通过。未执行完整跨平台、容器或 PostgreSQL CI。

## 运行文件与保护

当前程序：`/opt/stravia/releases/v0.6.4/masub-request-models-1/stravia-server`。
SHA-256：`53be51b71ca1b4a5804262bf1b46c49903da3a72f7a3d72658ac0f6800b0ab2e`。
旧定制程序仍保留在 `/opt/stravia/releases/v0.6.4/masub-responses/`，
SHA-256：`b4ef0ad9071f8feb0568eaa89cb0f4656ceea39675e7f4fbc90757d81e406914`。

GA 为 `0.6.4+masub.cc.1`，摘要
`19060d640bad0293f0240da0b91591e0f74ff191c64b5613a6532e1b1d33f776`；
Codex 为 `0.6.4+masub.responses.1`，摘要
`98985576c873d9fd35990f980d799d045955cda425321f77bf84232825fb0b08`。
Base/Claude Code 均为 0.6.4。未改变 SDK/WIT，无需重建这些已验证组件。

活动 manifest 保留 `positional_additional_tools` 和 `per_key_request_model_mapping_v1`。
后续升级须重放补丁、构建并测试目标程序，准备摘要匹配且包含全部活动标记的 staged 产物。
映射使用现有 SettingsStore，无本地 schema/migration；当前 SQLite migration 为 0045。

## 备份与清理

服务器保留两份热备份：

| 用途 | 路径 | SHA-256 |
| --- | --- | --- |
| 当前配置/映射 | `/var/backups/stravia/20261107T081338Z-manual.tar.gz` | `a8a172de3be29da5f19be8f39260f595d1d965798f08d407b50679248719195b` |
| 变更前 v0.6.4 | `/var/backups/stravia/20261107T075535Z-manual.tar.gz` | `4520d97f708fff1ee2f771038a233c606461194c1d9861e88185df9709e00484` |

两份均已下载并核验。系统配置、证书、必要工具、网络服务持久状态、GNU 运行库、当前/旧
原生程序及历史恢复资料已保存在运维机受限的 `private/stravia-recovery-20261107/`。
归档含凭据，不进入 Git；详情与恢复边界见 [RECOVERY.md](RECOVERY.md)。

所有待退役文件逐项核验本地副本后，清理 23 个旧文件、5 个旧目录及临时归档/失效别名。
包括旧 CPA/Sub2API 归档、旧快照服务器副本、v0.6.2/v0.6.3 版本目录、旧 staged 组件、
官方构建缓存和 CPA 临时插件目录。本轮实际回收 **471,170,048 bytes（约 449 MiB）**，
结束时空闲 **854,079,488 bytes（约 815 MiB）**。未清理活动数据、账号、当前缓存或网络服务配置。
测试压缩挂载和临时安装的七个工具包也已清理。磁盘数据为单次采样。

早期原生程序另在 `early-native-programs.tar.gz`，摘要
`5963a4130b2bab1d3e62b9bf9cea30e7b23d1386ec644f1f71d9867006f0c135`。
恢复 v0.6.2/v0.6.3 历史快照前，须从冷存储还原其版本目录及精确原生程序。
本次回退使用仍在服务器的旧 v0.6.4 程序和变更前快照：

```sh
stravia-maint restore /var/backups/stravia/20261107T075535Z-manual.tar.gz
```

完整恢复会同步数据、组件、补丁清单及程序指针；不要仅切换二进制读取另一状态的数据。
