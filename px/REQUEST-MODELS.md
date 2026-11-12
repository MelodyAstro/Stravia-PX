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
`wss://gateway.example.com/comments/responses`，备用为
`wss://gateway.example.com/comments/responses`。沿用原 Key 和映射后的短模型名。

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

## 验证和升级

使用 GitHub Actions 的原生构建和隔离 SQLite/PostgreSQL 验收。部署前校验源码提交、完整补丁及 SHA256SUMS；原生部署方法见 [运行手册](PLATFORM-PLUGIN-DEPLOYMENT.md)。

在自己的实例上运行 `ops/verify-responses-streams.py --host gateway.example.com --model gpt-6-luna` 验证完整 HTTP、WebSocket 和工具往返。合成请求不需要真实聊天捕获，报告仅包含状态、类型和耗时。凭据、运行清单和生产报告应存放在私有目录，不能提交仓库。
