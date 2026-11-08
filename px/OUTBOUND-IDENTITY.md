# Outbound request identity and TLS

Provider settings expose these fields in the existing generated form (English and Chinese).
They apply to the server-to-provider connection. Client UA, SDK, OS and TLS are not adopted.

| Provider | Existing identity | New identity controls |
| --- | --- | --- |
| OpenAI Codex | Server-generated `codex_cli_rs/0.162.1`, `version` and `originator` | `client_version`, `user_agent`, `originator`, official version sync |
| Claude Code subscription | Server-generated CLI 2.1.317, SDK 0.128.0 and Node v26.12.1 | `client_version`, `user_agent`, `package_version`, `runtime_version`, `runtime`, `os`, `arch`, `timeout`, `anthropic_version`, official version sync |
| Antigravity, including GA-Claude | Server-generated Antigravity CLI 1.3.5 | `client_version` and `user_agent`; use Antigravity identity for this transport |

Blank identity values preserve existing defaults. A custom UA may contain `{version}`;
it is expanded from the effective client version. A fixed UA without this placeholder
stays fixed. Codex uses the same version in model discovery and all HTTP/WSS identity
headers. Claude Code uses it in both the UA and request-body billing identity/fingerprint.
Authentication, provider account identity, session/turn IDs and dynamically derived
Anthropic betas remain owned by the existing provider implementation.

`auto_sync_client_version` is opt-in and defaults to false. With it enabled and no
manual `client_version`, allowance checks and model discovery refresh
the latest official stable release, at most once an hour per connection. Model calls
only read the cached result: they never query GitHub. Connection checks also read the
cache without querying releases or changing state. Refresh allowance or discover models
after enabling to populate the cache immediately. A manual version pin takes precedence.

Sources are the official GitHub release APIs for `openai/codex` (`rust-v` tags) and
`anthropics/claude-code` (`v` tags). Drafts, prereleases and malformed versions are
excluded. A release-list fallback handles an unusable latest release. Sync never
downgrades below the last successful or bundled version. A failed lookup preserves
the last good version and is throttled for an hour. No provider credentials or caller
headers are sent to GitHub. Cache state uses a namespaced section of the existing
connection-private JSON, preserving OAuth and continuation data.

Official CLI versions do not establish SDK, runtime or TLS fingerprints. These remain
explicit controls; version synchronization does not claim exact wire parity with an
unmeasured new official release. In particular GA-Claude uses Antigravity's upstream
protocol and must not be automatically assigned a Claude Code release number.

## TLS and HTTP controls

All three providers expose:

| Field | Values |
| --- | --- |
| `tls_min_version` / `tls_max_version` | `default`, `1.2`, `1.3` |
| `tls_cipher_suites` | Ordered, comma-separated Rustls cipher suite names; blank keeps defaults |
| `tls_key_exchange_groups` | Ordered list of `X25519MLKEM768`, `X25519`, `secp256r1`, `secp384r1`; blank keeps defaults |
| `http_version` | `auto` (HTTP/2 with HTTP/1.1 fallback) or `http1` |

Example TLS 1.3 configuration:

```json
{
  "auto_sync_client_version": true,
  "user_agent": "codex_cli_rs/{version} (Linux; x86_64)",
  "tls_min_version": "1.3",
  "tls_max_version": "1.3",
  "tls_cipher_suites": "TLS13_AES_128_GCM_SHA256,TLS13_CHACHA20_POLY1305_SHA256,TLS13_AES_256_GCM_SHA384",
  "tls_key_exchange_groups": "X25519,secp256r1",
  "http_version": "auto"
}
```

Supported TLS 1.2 suites are `TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384`,
`TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256`,
`TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256` and the corresponding
`TLS_ECDHE_RSA_WITH_*` variants. Custom suites must cover every enabled TLS
version. Unknown/duplicate groups and suites, reversed version bounds and header
control characters are rejected.

The host applies the policy to both HTTPS and WSS, with HTTP/1.1 ALPN for WebSocket.
Direct and proxy clients include the complete policy in their cache identity; a
different policy cannot borrow a pooled WebSocket established under an earlier one.
Certificate and hostname verification use the platform verifier. Existing proxy,
redirect-origin authorization and cancellation behavior remain active.

TLS is terminated and recreated by this server. These controls alter the real
ClientHello, but extension ordering, signature schemes and other Rustls internals
remain library-owned. Arbitrary JA3/JA4 strings and exact Chrome/Node fingerprint
emulation are not supported by this backend.

## Build and upgrade

Use the `PX Native Linux` workflow. It runs identity/version selection,
provider HTTP/WSS identity, and actual ClientHello tests, then builds the server and
Base/Codex/Antigravity/Claude Code components. Packages carry
`outbound_identity_tls_v1`. Keep this feature when preparing managed upgrades.
Provider options and the namespaced private state are included in existing backups.

Source-level configuration changes require the new host plus corresponding components.
Existing deployments do not gain these fields until that package is installed.

The deployment helper accepts both older packages and the new Claude Code component.
It preserves the installed Vendor inventory: a packaged Claude component is not installed
automatically. If `claude-code` is already installed, pin its candidate version in the
private specification's `vendor_versions` to upgrade it together with the host. A missing
pin or component fails before staging or cutover. Existing providers, credentials and
options retain their values; configure the new fields after the matching components load.
