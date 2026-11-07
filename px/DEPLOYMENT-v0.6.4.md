# v0.6.4 原生部署验证

> 这是首次升级到 v0.6.4 的历史记录。后续请求模型映射、当前程序、短名称配置、
> 私库 main 合并及最新回退快照见 [请求映射部署记录](DEPLOYMENT-request-models.md)。
> 文中的旧磁盘采样、模型名称要求和分支状态不代表后续状态。

2026-11-07（Asia/Pyongyang）已部署。官方基线为
`a100ef42031073fbdd779e4809fb83cac3a8b956`，构建源码提交为
`ef83425a7ae8b6d458321bb097a6ec749fea60c1`。本记录只包含状态与摘要。

## 当前运行版本

| 部分 | 版本 / 状态 |
| --- | --- |
| 原生主程序 | 0.6.4，`/opt/stravia/releases/v0.6.4/masub-responses/stravia-server` |
| Antigravity | `0.6.4+masub.cc.1`，Claude Code 兼容开关保持开启 |
| OpenAI Codex | `0.6.4+masub.responses.1` |
| Base / Claude Code 官方组件 | 均为 0.6.4 |
| 组件状态 | 四项均 ready |

Alpine/OpenRC，独立 GNU 运行库，非 root 单进程，只监听回环；没有使用 Docker。
保留 4 MiB 缓存预算、关闭单实例配置轮询、单个 Tokio/Rayon 工作线程及一天请求历史。
六个原 Key、十一个路由及 Nginx 配置指纹保持一致，SQLite 完整性与外键检查通过。

客户端 OpenAI Base 为域名加 `/comments`，Anthropic Base 为域名加 `/reviews`。
GPT-Private 需要完整模型名，例如 **`private/gpt-6-luna`**；不要使用裸模型名。
Team 和官方 Claude 空池的 Key 继续禁用，未借用 Private 账号替代 Team。

## 实际验证

- 完整捕获的 Codex 请求包含 11 个 input item 和 `additional_tools`。只修正模型路由名后，
  源站 HTTP 200、两个公网域名 HTTP 200 / WebSocket 101 均收到完整 `response.completed`。
- 非默认 `px_diag.echo` 命名空间的调用身份、参数及工具结果继续请求全部通过。
  默认 `functions` 命名空间可省略输出字段；断言使用非默认命名空间避免误判。
- Claude 使用合成的 CC 请求结构：7 条消息、38 个工具、32000 输出上限、adaptive
  thinking。Sonnet 与 Opus 均 200/end_turn，字典参数工具调用和结果往返也通过。
  本轮未重放旧的 CC 原始聊天副本；该副本此前已清理。
- Gemini Chat Completions 完整结束，HTTP 200；本次样本总耗时 **54.682 秒**。
  不据此宣称延迟已改善，也未更改模型或思考设置来缩短样本。
- 两域名 WebUI 和 JS 入口 200，HTML no-store、哈希静态资源 immutable。
  六组 Key 的模型可见性符合配置；无 Key 401，空组与跨组调用 403。
- 同版本 `stravia-maint upgrade v0.6.4` 返回 already_current，保留两个补丁。
- 仓库 `px/ops/verify-responses-streams.py` 的合成 HTTP、WebSocket、命名空间工具与
  结果继续四项也已实际执行通过，后续复验不依赖已删除的原始聊天捕获。
- 643 个协议测试、7 个 GA 兼容回归、8 项隔离升级、3 项隔离恢复及 1 项快照测试通过。
  未运行上游完整多平台/容器 CI。

主域名升级后的首次页面检查曾未识别到 WebUI；最终两个域名页面、静态资源及完整
Codex HTTP/WebSocket 复验均通过。上述为服务端主动验证，未将客户端用户确认计入结果。

临时 Debug 已关闭，Debug Trace 与临时原始请求副本已清理，正常请求历史保留。
最后资源采样：PSS 83,515 KiB，SwapPss 66,552 KiB；磁盘空闲约 **419.7 MiB**。
这些为单次采样，不是负载或延迟保证。相同旧插件文件经摘要校验后以硬链接去重，
保留全部原路径、旧版本主程序及完整回退备份。

## 产物与恢复

| 产物 | SHA-256 |
| --- | --- |
| 原生主程序 | `b4ef0ad9071f8feb0568eaa89cb0f4656ceea39675e7f4fbc90757d81e406914` |
| GA 兼容组件 | `19060d640bad0293f0240da0b91591e0f74ff191c64b5613a6532e1b1d33f776` |
| Codex 兼容组件 | `98985576c873d9fd35990f980d799d045955cda425321f77bf84232825fb0b08` |
| 升级前完整备份 | `7e2f3b67c8c87aa9e614228413543353f0e33f2d8ece932c34e1c8ac65664fab` |

升级前归档 `/var/backups/stravia/20261106T213315Z-before-v0.6.4.tar.gz`
已下载到运维机并核验 SHA-256。需要回退时运行：

```sh
stravia-maint restore /var/backups/stravia/20261106T213315Z-before-v0.6.4.tar.gz
```

回退同时恢复 v0.6.3 数据、旧补丁及程序指针。不要只切换二进制读取新版数据。
后续已将 v0.6.3 原生程序转存运维机冷存储，执行上述历史回退前必须先校验并还原
其精确二进制；最新记录提供归档摘要和原路径。本次请求映射变更的回退使用服务器
保留的旧 v0.6.4 定制主程序，不需要降到 v0.6.3。
本次版本迁移仍停留在 SQLite migration 0045，没有新增本地 migration。

后续升级先重放两个补丁，重建主程序、Base 和独立插件，验证后准备目标版本的两份
staged 清单。维护工具会校验摘要、备份、切换并检查 ready；缺少匹配补丁对时停止。
最低 400 MiB 空闲检查仅是保护下限；若新版本产物或快照变大，应先重新估算空间。
候选缓存不进入回退快照，活动数据、补丁源码差异、组件和清单均进入快照。

首次部署时改版分支为 `px/v0.6.4`，`main` 当时保留官方基线；后续用户要求将改版
合并至 `main`，以最新请求映射部署记录为准。`upstream` 继续指向官方仓库。
GitHub 登录由运维机 Windows 凭据管理器持久保存，凭据不在仓库或服务器脚本中。
已用禁止交互的模式验证私库读取及推送；授权撤销或失效时仍可能需要重新登录。
