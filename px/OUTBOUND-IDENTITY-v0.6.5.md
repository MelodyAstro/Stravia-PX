# Client identity and recognition in v0.6.5

`identity_source` selects `configured` (default) or `client` on Codex, Claude Code
and Antigravity connections. Configured mode uses cloud UA/version/SDK/environment
settings. Client mode admits request-scoped software identity headers from a
bounded allowlist; missing, ambiguous or malformed values keep configured
defaults. Credentials, cookies, routing, forwarded IPs and session IDs cannot be
replaced through this mode. Caller identity is never persisted across accounts.

`auto_sync_client_version` optionally refreshes official stable releases during
allowance/model discovery, at most hourly. Manual pins take precedence. Inference
and connection checks never fetch releases. Failed refreshes retain the last good
version; client mode uses the request's version without updating shared state.
Version synchronization alone does not establish a matching SDK/runtime/TLS
capture. Antigravity uses its own protocol/version, including for GA-Claude.

`tls_shape` and `tls_ws_shape` independently select `vendor` (plugin default),
`client` (known preset inferred from UA), or `default`, `claude-code`, `bun`,
`bun-1-3-14`, `antigravity`, `codex-http`, `codex-ws`, `command-code`, `devin`, `grok`.
Unknown UAs have no inferred preset. The server recognizes declared Claude Code,
Codex and Antigravity characteristics; it cannot authenticate a client by UA.

The official v0.6.5 Rama/patched-Rustls transport controls ClientHello, HTTP
field layout and HTTP/2 settings. Existing min/max TLS versions, ordered cipher
suites/groups and HTTP version overrides remain supported with platform
certificate/hostname verification and isolated connection reuse.

TLS terminates at the CDN/proxy and is recreated upstream. HTTP header passthrough
cannot relay the original TLS handshake. Captured presets cover their recorded
release/platform and do not guarantee parity with every official client, TCP/IP
behavior, timing or arbitrary JA3/JA4 values. This mode retains protocol encoding,
the Claude compatibility switch, routing and authorization.

Enable the existing **180-second request recognition** on a particular key to
inspect declared family/version, SDK environment, suggested shapes, stream and
transport, item/tool counts, normalized output limit, thinking budget and effort.
Different identities/parameters remain separate samples. Capture is bounded,
expires automatically and can be cleared. No chat body, tool schema, authorization
or cookie is retained in these summaries.

Design references: [CLIProxyAPI configuration](https://github.com/router-for-me/CLIProxyAPI/blob/main/config.example.yaml)
and [sub2api identity service](https://github.com/Wei-Shaw/sub2api/blob/main/backend/internal/service/identity_service.go).
Transport shapes come from StraviaPlatform's recorded profiles. This implementation
keeps caller identity request-scoped instead of writing permanent account fingerprints.
