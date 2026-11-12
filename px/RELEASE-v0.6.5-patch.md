Strinova 基于 [StraviaPlatform v0.6.5](https://github.com/Stravia-AI/StraviaPlatform/releases/tag/v0.6.5)，保留上游来源与 AGPL-3.0-only 许可证。

- 保留 Claude Code / GA-Claude 兼容开关、Codex Responses 与自定义工具、分组 Key 和云端模型映射。
- 增加独立模型映射插件，适配 0.6.5 请求钩子和 HTTP/WebSocket TLS；保留可配置或透传的客户端身份参数。
- 增加 Codex / Claude 官方 reset 次数读取、确认后手动 reset，以及默认关闭的账号累计本站消费上限。缺少逐次扣费明细时按官方费率保守估算，明确标注“估算”；预留按完整上下文计算，小额度上限可能提前拒绝请求。
- 支持 Codex OAuth 的 sub2api、Cockpit Tools、CLIProxyAPI/CPA 和官方 auth.json 导入；支持 Antigravity/Google RT JSON 导入及确认下载备份。
- 完善模型选择框圆角边框、Strinova 品牌及中英文文案。CI 自动保留最新 25 条任务记录，并复用编译缓存。

提供 Linux x86_64 原生程序（glibc ≥ 2.35）、独立 Wasm 组件、完整源码补丁及四层 patch、构建来源清单和 SHA256SUMS。原生包内组件与主程序来自同一通过测试的源码提交。Alpine 部署需已有兼容 glibc 运行时。

兼容改动分为宿主补丁与可热更新组件；未经宿主补丁适配的官方二进制不能直接加载这些新增功能。升级前请备份完整数据与配置，失败时同时恢复对应旧程序和数据。RT 备份包含可登录账号的凭据，请私下保管。

测试和使用边界见 [维护说明](https://github.com/MelodyAstro/Strinova/tree/main/px)、[账号额度控制](https://github.com/MelodyAstro/Strinova/blob/main/px/ACCOUNT-CONTROLS.md)及[凭据迁移说明](https://github.com/MelodyAstro/Strinova/blob/main/px/CREDENTIAL-IMPORT.md)。

合成请求验收中，Codex OAuth 的 HTTP、WebSocket、命名空间函数和自定义工具往返通过；Claude 的中性 system 合成请求（7 条消息、38 个工具、32000 输出上限、adaptive thinking）以及字典工具往返通过。仍有以下限制：

- 含特定 `Anthropic` 身份描述的 system 请求仍可能被 Antigravity 上游返回 `429 RESOURCE_EXHAUSTED`，即使额度面板显示有余额、兼容编码已经执行。原兼容开关已保留，不能据此保证所有 Claude Code 请求成功；该失败样本未计入通过结果。
- OAuth 连接在上游返回 401 后应重新授权；账号分组保持隔离，不会借用 Private 凭据。这不代表所有同类账号均不可用。

Native Linux release built from upstream StraviaPlatform 0.6.5 with the Strinova compatibility components, model mapping plugin, account controls and OAuth credential migration. Source attribution and AGPL-3.0-only licensing are preserved. Verify SHA256SUMS and build-manifest.json before installation; restore both data and the matching program when rolling back.

Live validation passed for Private HTTP/WebSocket and function/custom-tool round trips, and for neutral-system Claude fixtures including 38 tools and tool results. Known limitation: Antigravity can still return upstream 429 for specific system identity descriptions despite available quota and an active compatibility switch. OAuth connections returning upstream 401 require reauthorization while retaining their account-group isolation. These failing cases are not reported as passing.
