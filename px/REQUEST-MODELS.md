# API Key 请求模型映射

客户端发送的模型名称可以按 API Key 映射到独立的内部路由。
同一短名称可以分别指向 Private 和 Team；最终路由仍须通过 Key 的模型权限检查。

配置使用现有设置存储，不修改数据库结构。映射是精确匹配、单次解析，目标使用稳定的
内部路由 ID。关闭后恢复原有路由解析；映射目标失效时不会自动改用其它账号。

限时识别只保存模型名称、协议、传输方式及消息/工具数量，不保存正文、密钥或 headers。
识别结果提供候选映射，管理员保存后才生效；识别窗口最长三分钟。

## 配置入口

API Keys → 编辑一个已有 Key → 请求模型映射。列表的编辑侧栏也提供这个入口。
输入客户端实际发送的名称，选择此 Key 有权访问的目标，开启并保存。
例如 GPT-Private 的 `gpt-6-luna` 指向内部 `private/gpt-6-luna`；GPT-Team 可以独立
将同名请求指向 `team/gpt-6-luna`。这不会把目标的权限授予其它 Key。

映射启用后，`/comments/models` 展示映射名称；原完整名称仍可用于诊断。
Responses HTTP/SSE、WebSocket、Chat Completions、Anthropic 等使用共享的路由解析，
媒体能力查询也解析相同映射。配置只改变目标选择，不改写消息、工具定义或上游模型名。
响应和续接仍使用客户端请求名称。

GPT Responses 的 WebSocket 完整地址为
`wss://masub-ex.retlinks.top/comments/responses`，备用为
`wss://masub-ex.yuhatze.org/comments/responses`。沿用原 Key 和映射后的短模型名。

请求名称大小写敏感，允许 1–160 个 ASCII 字母、数字和 `._-/:`，每个 Key 最多 128 条。
同一 Key 的请求名称不能重复，也不能覆盖内部 Route key；不同 Key 可使用相同名称。
目标保存的是稳定的路由 key，重命名目标的公开模型名不会改变绑定。目标删除、停用或
权限被收回时，请求不会换用其它账号。映射关闭后恢复原有名称解析；关闭时可保留待修复
的旧目标，以便随时停用。禁用且没有模型绑定的空 Key 不提供映射候选。

## 限时识别

点击「开始三分钟识别」，在客户端用相应 Key 发请求。模型不存在导致的 404 也可以
识别；未通过 Key 认证的请求不会进入该 Key 的记录。协议解码失败的请求不在此识别范围，
这不是任意请求格式的转换器。

每条样式仅保存：模型名、协议、HTTP/WebSocket、流式标记、输入条目数、工具数及次数。
工具数包含 Responses 的位置工具头；工具名称、参数 Schema、聊天正文和 headers 不保存。
同一样式合并计数，最多保留二十种，超过后明确显示未保留数量。最多十六个识别会话。
三分钟后停止收集，结果十五分钟后过期；可以提前停止或清空，重启也清除记录。

仅在已授权目标中按公开名称、去除账号前缀后的名称或实际配置的上游模型名精确识别。
匹配唯一时预选目标，有歧义或无法识别时由管理员选择；不猜测相似版本，不自动保存或
扩大权限。点击「添加识别名称」只编辑草稿，最终仍须保存。

## 管理 API 与存储

以下路径在既有管理员认证、同源/CSRF 防护内，不能用普通客户端 Key 修改：

```text
GET    /api/v1/api-keys/{id}/request-models
PUT    /api/v1/api-keys/{id}/request-models
POST   /api/v1/api-keys/{id}/request-models/capture
DELETE /api/v1/api-keys/{id}/request-models/capture
```

GET 返回 `config`、`eligible_models`、`capture` 和 `suggestions`。PUT 示例：

```json
{
  "enabled": true,
  "mappings": [
    {"request_model": "gpt-6-luna", "target_model_id": "<eligible_models 中的 id>"}
  ]
}
```

POST 使用 `{"enabled":true}` 开始新的三分钟会话，`false` 停止但保留结果；DELETE
同时停止并清空。删除 Key 时清空其识别和映射。识别数据只在进程内存中。

映射存在 `settings` 的 `px_request_models:<key-id>`，只能经专用 API 写入和校验；
通用 settings API 拒绝此前缀。使用 SQLite/PostgreSQL 共同的 SettingsStore，无新增表或
migration。完整数据快照自然包含映射配置。

## 构建与升级

本功能只改 Core、原生 HTTP 管理路由及 WebUI；现有 GA/Codex 协议补丁继续保留。
重新构建 WebUI 和原生主程序，独立 Wasm 组件在 SDK/codec 未变化时可复用已验证产物。
先验证隔离的 Key 路由、权限撤回、模型列表、请求识别和重启持久化，再验证公网完整
HTTP/WebSocket 与工具结果往返。

Responses 受管理主程序的 manifest 增加：

```json
{"features":["positional_additional_tools","per_key_request_model_mapping_v1"]}
```

后续 staged 主程序必须保留活动 manifest 的全部 feature 标记，摘要和源码差异也必须
匹配。维护工具在修改运行状态前拒绝缺项，避免只保留协议补丁却丢掉云端映射。

## 运维脚本

- `ops/package-request-models.py --repository <checkout> --output-directory <private-dir>`
  打包 GNU 主程序和构建出的不可变网页资源；`--tests` 打包隔离测试二进制。
- `ops/run-native-request-model-tests.py` 在本实例独立 GNU 运行库中运行九项测试，
  使用临时数据库和本地模拟上游，不调用生产账号。测试包经分片传输后，以只读
  SquashFS 挂载运行，避免展开大型测试二进制；挂载和临时目录在结束时清理。
  需先准备 root 私有测试产物清单，以及临时 `squashfs-tools-ng` 和内核模块。
- `ops/deploy-v064-request-models.py` 是本次 v0.6.4 的一次性切换脚本，校验旧版本、
  摘要和功能标记，完整备份后再切换；失败恢复。它不是通用未来版本升级入口。
- `ops/verify-request-models-live.py` 在本实例用合成请求验证短名称、临时自定义名称、
  两域名 HTTP/WebSocket 与工具往返、权限、识别和静态资源。会实际调用上游；
  需同目录对应脚本在服务器命名为 `/root/verify-stravia-responses-streams.py`，
  以及既有 root 私有认证助手和凭据。完成后恢复原映射并清空识别。
  同目录需保留 `stravia_verify_webui.py`，网页探测使用浏览器 User-Agent。
- `ops/verify-claude-code-compat.py` 验证 Sonnet、Opus 的 7 消息/38 工具/32000 上限
  合成 CC 请求和字典工具结果往返；保持兼容开关开启，不需要真实聊天捕获。

最终 Linux 测试二进制构建对 `stravia-core` 单独使用 `opt-level=0` 和
`codegen-units=16`；九项回归完整保留。该配置生成的是测试产物，不作为生产
主程序发布。原生生产主程序仍使用默认 release 优化。等价 Cargo 参数为：

```sh
cargo test --locked --release --target x86_64-unknown-linux-gnu \
  --config 'profile.release.package.stravia-core.opt-level=0' \
  --config 'profile.release.package.stravia-core.codegen-units=16' \
  -p stravia-core --lib --no-run
```

GNU 目标的 glibc 最低版本需要匹配服务器的独立运行库；跨平台构建使用对应 Zig linker。

平台单个 Artifact 上限为 100 MiB。`ops/upload-test-parts.cjs <private-directory>`
将测试包分成 64 MiB 分片，调用同目录的 `upload-build-artifact.cjs` 上传，并只把
分片 ID、大小和摘要写入清单。私有目录需有 `stravia-access.json`、打包产物和清单。
每片上传凭据回执也只留在该私有目录，不能提交。服务端从分片流直接生成压缩镜像，
不额外拼接整包或展开 ELF。测试通过后移除临时安装的测试工具。

运维脚本中的清单、产物和报告放在 root 私有目录，不提交仓库；完整备份也包含凭据。
本次通过 Artifact 上传的构建包在验证并使用后标记到期，由平台正常 GC 清理；
不对用户上传文件做清理。打包产物另留在运维机，回退使用旧原生程序和完整快照。
未来编译 LXC 只负责产出和校验原生文件，运行服务器无需安装编译工具。
