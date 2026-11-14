# Public Strinova SDK

The public build base exposes the platform plugin SDK and runtime contracts.
A plugin is a Wasm component installed through the platform plugin API; it
receives the host capability set declared by the SDK and cannot assume
vendor-specific implementation details.

Use the SDK crates under backend/crates/stravia-platform-plugin-sdk and
backend/crates/stravia-platform-plugin-runtime from the matching public
release. Keep plugin IDs, versions, and exported capability names stable.
Build components with the release workflow so the generated digest and
install metadata are recorded together.

The framework host and vendor implementations are applied only in CI from the
private patch repository.
