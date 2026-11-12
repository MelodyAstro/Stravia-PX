# Strinova maintenance

Strinova is based on [StraviaPlatform v0.6.5](https://github.com/Stravia-AI/StraviaPlatform), with upstream attribution and the AGPL-3.0-only license preserved.

The fork adds Claude Code compatibility controls, Codex Responses/custom tools, per-Key model aliases, platform plugins, client identity configuration, account reset controls, estimated site spending limits, and OAuth credential migration.

- [Plugin boundaries and upgrades](PLUGIN-MAINTENANCE.md)
- [Platform plugins](PLATFORM-PLUGINS.md)
- [Request model aliases](REQUEST-MODELS.md)
- [Client identity and TLS](OUTBOUND-IDENTITY-v0.6.5.md)
- [Account reset and spending controls](ACCOUNT-CONTROLS.md)
- [Credential import and RT backup](CREDENTIAL-IMPORT.md)
- [Native CI builds and caches](CI-BUILDS.md)
- [Native deployment runbook](PLATFORM-PLUGIN-DEPLOYMENT.md)
- [Public-source privacy rules](PRIVACY.md)

Use the domain configured by your administrator. For a site configured with the optional public prefixes:

| Surface | Example |
| --- | --- |
| OpenAI HTTP Base | `https://gateway.example.com/comments` |
| Responses WebSocket | `wss://gateway.example.com/comments/responses` |
| Anthropic Base | `https://gateway.example.com/reviews` |

The paths are reverse-proxy configuration examples. The native host's canonical protocol paths remain available on its configured listener. With request mapping enabled, separate Keys can map the same short model name to different authorized routes; isolation is still enforced.

CI produces a source-bound native GNU package, independent Wasm components, complete source patch, four reviewable patch layers, a build manifest and checksums. Native deployment does not require Docker; Alpine needs a compatible isolated GNU runtime. Keep the complete data snapshot and its matching previous program for rollback.

Public documentation contains examples only. Actual domains, SSH endpoints, account identifiers, production topology, backup inventories and credentials belong in private operational storage. The public runbooks do not describe any specific deployment's current state.
