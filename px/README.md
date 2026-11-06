# Stravia-PX

自用的 Stravia 补丁与原生部署维护记录。基线为官方 **v0.6.4**
（`a100ef42031073fbdd779e4809fb83cac3a8b956`），保留官方源码历史及 AGPL-3.0 许可证。

## 当前补丁

- **GA / Claude Code**：保留 `claude_code_compat` 开关，只作用于 Antigravity 的
  `claude-*` 模型；保留工具参数 JSON Schema，兼容 system 文本投影。组件版本为
  `0.6.4+masub.cc.1`。Gemini 的投影不变。
- **Codex Responses**：接受、校验并原位保留 `additional_tools`，完整传递 namespace、
  函数 schema、自定义工具格式和位置。主程序及 Codex 插件必须同时更新；仅更新其中
  一个会继续拒绝请求或丢失工具定义。组件版本为 `0.6.4+masub.responses.1`。
  无法表达该工具位置语义的跨协议目标会明确拒绝。
- 包含 v0.6.4 官方的签名思考块顺序修复，更新主程序时同时重建受影响插件。

Private 模型使用完整路由名，例如 `private/gpt-6-luna`；Team 仍使用 `team/`。
客户端 Base URL 的 OpenAI 前缀为 `/comments`，Anthropic 前缀为 `/reviews`。

## 原生构建

遵循上游 `AGENTS.md`、`rust-toolchain.toml` 和 `Taskfile.yml`。构建不需要 Docker：

```sh
# 安装固定版本 Rust、Bun 和 Task；准备 wasm32-wasip2 目标。
rustup target add wasm32-wasip2

# 完整协议回归；实际执行 643 个测试。
cargo test --locked -p stravia-protocol-codec --lib

# 上游标准流程生成 WebUI、内嵌 Base 组件和原生主程序。
task build:server

# 两个受管理的独立补丁组件必须一起重建。
cargo build --locked --release --lib --target wasm32-wasip2 \
  -p stravia-vendor-antigravity -p stravia-vendor-codex
```

内存有限的构建机器应设置 `CARGO_BUILD_JOBS=1`。Linux GNU 产物的 glibc 要求须与
目标运行库匹配；本实例使用 Alpine/OpenRC 和独立 GNU 运行库，以非 root 用户运行。
原生服务不通过 Docker 部署。

## 升级和恢复

`ops/stravia-maint.py` 部署为 `/usr/local/sbin/stravia-maint`。账号读取助手为
`/root/stravia-client.py`；它从服务器 root 私有文件读取管理账号。仓库不包含账号、
API Key、OAuth 凭据、生产数据库、真实聊天请求、Debug Trace 或完整备份。

```sh
stravia-maint backup
stravia-maint upgrade vX.Y.Z
stravia-maint restore /var/backups/stravia/<snapshot>.tar.gz
stravia-cc-compat status
stravia-cc-compat on
stravia-cc-compat off
```

先阅读目标官方版本变更，再将补丁重放到目标源码，并重建、测试主程序与插件。升级
必须保留账号隔离和分组 Key。新主程序及插件就绪后还需验证完整 HTTP/SSE、WebSocket
响应，以及工具调用/结果往返；握手成功不能替代完整响应验证。

GA 补丁清单位于 `/opt/stravia/compat/manifest.json`，目标版本组件放在
`/opt/stravia/compat/staged/vX.Y.Z/`。Responses 清单位于
`/opt/stravia/responses-compat/manifest.json`，目标主程序和 Codex 组件放在对应的
`staged/vX.Y.Z/`。维护工具验证文件摘要、创建完整数据快照、切换程序并安装补丁；失败
时恢复数据与旧程序指针。不得仅切换旧程序读取较新的数据库。

Responses 目标目录至少包含主程序、Codex Wasm、`source.patch` 和 `manifest.json`：

```json
{
  "upstream_tag": "vX.Y.Z",
  "vendor_id": "openai-codex",
  "version": "X.Y.Z+masub.responses.1",
  "host_binary": "stravia-server",
  "host_sha256": "<SHA-256>",
  "component": "stravia-vendor-openai-codex.wasm",
  "sha256": "<SHA-256>",
  "source_patch_sha256": "<SHA-256>"
}
```

主程序在同一文件系统内以硬链接切入版本目录，避免重复占用近 190 MiB；已验证的官方
压缩包保留，展开的未使用官方主程序会移除。运行中的旧版本程序与完整快照保留。
快照保留运行中的数据、补丁组件、源码差异及清单；可重建的 `staged/` 候选目录不进入
快照，避免恢复旧版本时又展开一份近 190 MiB 的候选程序。

若没有匹配目标版本的补丁对，维护工具会在修改运行状态前停止。官方以后修复了相同
问题时，仍须确认主程序和插件均支持相同语义，再准备目标清单。

## 验证与隐私

- v0.6.4 的 643 个协议测试及 7 个 GA 兼容测试通过。
- 8 项隔离升级测试覆盖补丁保留、首次安装、摘要篡改、其它插件更新及失败回退。
- 3 项隔离恢复测试覆盖元数据同步、补丁出现前的快照及篡改检查。
- 快照测试验证活动数据与补丁文件完整保留，候选缓存排除，摘要与旧程序指针有效。
- 生产验证与备份摘要在部署完成后另行记录；源码测试不代表线上请求已成功。

临时 Debug 必须限时开启并恢复关闭，只报告字段/类型和测试状态；完成后清理临时
原始请求副本。真实请求内容和密钥不进入 Git。

协议依据：[OpenAI additional_tools](https://developers.openai.com/api/docs/guides/tools-tool-search)。
