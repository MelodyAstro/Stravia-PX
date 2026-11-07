# Native custom-tool fix and Team slot — 2026-11-07

The `503: Model has no eligible Target` incident followed repeated protocol
errors on `custom_tool_call`. The upstream had generated a valid native custom
tool item, but the old parser rejected it and the Target entered cooldown.
The native custom-call parser, history replay, streaming formatter and visible
output commit fix are now deployed on the v0.6.4 maintenance line.

## Running build

- Release path: `/opt/stravia/releases/v0.6.4/masub-custom-tools-1`.
- Host SHA256: `93988500584750e2dd592e06ec73b4a5a8b62eeed0f88b264b6a1f1db7fa2924`.
- Runtime source revision: `8396fef622da8ea012901f2894daec9f1799c85e`.
- Codex component: `0.6.4+masub.responses.2`, SHA256
  `02bff2c82eb34f3a41cf9174683e17072a993da59923b3b036ba20bd3f01b601`.
- Embedded Base SHA256:
  `a01d4349dd2c11ad9d0a63c7764f256dc7c3186de5780ccca0d5c1194cbee992`.
- GA component stays `0.6.4+masub.cc.1`, SHA256
  `19060d640bad0293f0240da0b91591e0f74ff191c64b5613a6532e1b1d33f776`;
  `claude_code_compat` remains enabled.
- Managed features: `positional_additional_tools`,
  `per_key_request_model_mapping_v1`, `native_custom_tools_v1`.
- Native Alpine/OpenRC deployment with the existing GNU runtime; no Docker.

Both active GPT Providers now explicitly store
`wss://chatgpt.com/backend-api/codex/responses` as the upstream WebSocket URL.
Client OpenAI Base remains `/comments`, Anthropic Base remains `/reviews`.
See [upstream WebSocket configuration](UPSTREAM-WEBSOCKET.md).

The renewed `GPT-Team-5X-SOLT-1` has eight separate Team routes and eight bare
model aliases on its original GPT-Team Key. Private bindings are preserved.
The All group explicitly includes both pools, keeping its existing bare aliases
pointed at Private. See [Team slot configuration](TEAM-SLOTS.md).

## Verification

- 651 protocol unit tests and 7 GA compatibility unit tests passed locally.
- 2 native Linux custom-stream delivery tests and all 9 native request-mapping
  regressions passed with isolated temporary data.
- 48 successful public GPT responses across both hostnames and both account
  groups: 32 custom-tool call/result responses, including namespaces and grammar
  inputs over HTTP and WSS; 16 normal Responses/function-tool responses.
- Both groups list the expected eight bare names. Cross-account canonical
  requests return 403. Existing Keys were preserved.
- Four active components are ready, current host/component digests match,
  SQLite integrity checks passed, Debug remains off, and Nginx is unchanged.
- Initial GPT verification encountered its existing five-hour quota guard at
  100% usage. The tests passed after the upstream window reset naturally;
  guard settings and retry budgets were preserved.
- GA live replay returned upstream 429 with `Individual quota reached` and an
  approximately 62-minute reset window. It did not complete at this checkpoint.
  The installed GA component and compatibility switch are unchanged; its unit
  tests passed. This quota result is separate from the fixed GPT parser error.

Future builds use [GitHub-hosted native CI and compiled tool caches](CI-BUILDS.md).
CI packages carry their own source revision and digests; their bytes may differ
from the currently deployed cross-compiled package. Do not confuse a later CI
artifact with the exact host digest above.

## Recovery

Before-change snapshot:
`/var/backups/stravia/20261107T114546Z-manual.tar.gz`, SHA256
`1d4d59836a8c677c8bf0f65c0181643b7f09f73730267ae3fced06e9ac6698f3`.
Its program is `/opt/stravia/releases/v0.6.4/masub-request-models-1`, SHA256
`53be51b71ca1b4a5804262bf1b46c49903da3a72f7a3d72658ac0f6800b0ab2e`.

Current complete snapshot after configuring Team:
`/var/backups/stravia/20261107T120716Z-manual.tar.gz`, SHA256
`4a2e0320764b62976a6226eb4a860660621868a52a1282fbb3996faec1b26c11`.
Local recovery files are kept under `private/stravia-custom-tools-20261107/`.
The host/component package SHA256 is
`8ca584cc6a18b1a1ba99fc1295949d0ee3d0a663abe828065ec0c2ecfb080c94`.
The complete patch uses official v0.6.4 commit
`a100ef42031073fbdd779e4809fb83cac3a8b956` as its base, SHA256
`843aff49f68208128ef51c0d896771e83c98aafb8514d48f81305c659a205416`.

Restore through `stravia-maint restore <archive>` so data and active components
are restored together. Restoring the before-change snapshot also restores the
previous Key/model configuration; the Team slot completion can be reapplied
from its root-only configuration snapshot and the saved current archive.
