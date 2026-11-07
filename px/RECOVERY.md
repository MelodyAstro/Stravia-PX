# 配置备份与恢复

2026-11-07 初期恢复资料位于 `private/stravia-recovery-20261107/`；包含本次自定义工具修复和 Team 分组的最新完整备份位于 `private/stravia-custom-tools-20261107/`，详见 [当前部署记录](DEPLOYMENT-custom-tools.md)。
目录权限限定为当前用户、SYSTEM 和管理员。配置、OAuth、API Key、服务证书和完整数据
只保存在该私有目录；仓库仅保留脚本、状态和摘要。

| 本地文件 | 内容 |
| --- | --- |
| `snapshot-current.tar.gz` | 请求模型映射阶段的完整 Stravia 数据及活动补丁 |
| `snapshot-before-request-models.tar.gz` | 本次变更前完整 v0.6.4 状态 |
| `server-config-and-tools.tar.gz` | `/etc`、服务/维护工具、管理凭据、网站静态资源及网络程序 |
| `network-state.tar.gz` | Hypernet/Cloudflared 持久状态与相关证书资料 |
| `gnu-runtime.tar.gz` | 独立 GNU 运行库 |
| `current-native-host.tar.gz` | 当前主程序与不可变 WebUI 资源 |
| `previous-native-host.tar.gz` | 上一个已验证的 v0.6.4 定制主程序 |
| `early-native-programs.tar.gz` | v0.6.2/v0.6.3 精确原生程序及原路径 |
| `stock-v064.tar.gz` | 官方 v0.6.4 GNU 发布归档 |
| `retired-files.tar.gz`、`retired-*` | 历史配置/快照、旧版本组件和退役文件 |

`SHA256SUMS`、`cleanup-local-verified.json` 和 `cleanup-plan.json` 提供完整性与原路径；
`verification-reports.json` 记录实际运行的验证。退役归档的成员也已逐项校验摘要。

同机回退使用 [当前部署记录](DEPLOYMENT-custom-tools.md) 中保留的热备份与对应程序。
新机恢复先准备匹配的 Alpine/OpenRC 依赖、GNU 运行库及对应原生程序，核对并恢复系统配置，
再恢复完整数据与活动组件清单、设置服务属主和程序指针。系统配置包包含 SSH、网络和证书
配置，应按目标机器逐项恢复。完整数据包含加密凭据及其服务配置，不应只复制数据库文件。

历史 v0.6.2/v0.6.3 版本目录已从服务器移除。恢复这些快照前须先还原 `retired-files.tar.gz`
中的版本目录，以及 `early-native-programs.tar.gz` 的对应原生程序；维护工具会检查目标程序存在。

## 再次备份或清理

`stravia-maint backup` 创建一致数据快照并恢复服务。`ops/create-recovery-capsules.py`
在服务器 root 环境备份配置/启动工具与 GNU 运行库。归档应下载至受限目录并核验后再清理副本。

本次一次性清理工具为 `plan-v064-server-cleanup.py`、`archive-v064-retired-files.py` 和
`apply-v064-server-cleanup.py`：先生成计划，复用已有本地相同摘要的档案，其余文件及目录归档；
本地逐项验证后才上传清理回执并删除。执行器再次检查文件摘要、活动进程引用、热备份、
程序与配置指纹；活动数据、运行库、当前和回退版本均受保护。这些工具绑定本次路径和计划，
不是按年龄自动删除服务器文件的通用任务。

## 自定义工具修复后的清理

新快照已下载并校验，离线检查确认 SQLite 完整、6 个 Key、19 条路由和三个分组映射均在。
服务器保留 `20261107T114546Z-manual.tar.gz`（修复前）和
`20261107T120716Z-manual.tar.gz`（修复与 Team 配置后）两份热备份，以及当前、前一个原生程序。

较早的 `20261107T075535Z`、`20261107T081338Z` 备份及其 root 硬链接已在再次核对本地
摘要后清理。旧 `/opt/stravia/releases/v0.6.4/masub-responses/stravia-server` 也已转为冷备份；
对应字节位于初期备份目录的 `previous-native-host.tar.gz`。恢复指向该旧目录的历史快照前，
须先将该原生程序按记录路径还原。此次清理释放约 379 MiB，当前与前一修复版本均未删除。
