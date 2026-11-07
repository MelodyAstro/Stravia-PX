# GitHub-hosted native builds

Use **Actions → PX Native Linux → Run workflow** on `main` or a reviewed maintenance branch.
Pushes to `main` rebuild when backend, frontend, toolchain, lock files, or this workflow change.
The workflow runs on GitHub-hosted Ubuntu 22.04 (glibc 2.35), without Docker.
No local compilation or production credentials are required.

The workflow builds the embedded Base component, managed Codex and GA components,
runs protocol and targeted Core regressions, builds the optimized server, and
uploads a private artifact named `stravia-px-linux-x86_64-gnu-<full commit>`.
Artifacts are kept for 14 days. Rust build caches are reused on later runs.

Download the successful run's artifact, verify `SHA256SUMS`, and check the exact
`source_commit`, run URL and file digests in `build-manifest.json`. The package
contains the host, Base/Codex/GA Wasm components, immutable WebUI assets, and the
complete patch from the recorded official base. Keep a verified package with
local recovery backups before the artifact expires.

Deployment remains a separate maintenance action: back up the server, preserve
all managed feature flags, install matching host/components, and run complete
HTTP/SSE and WSS tool round trips. CI has no SSH key, account tokens, API keys,
or production data and cannot deploy automatically.

The original full upstream CI matrix stays available for pull requests and
manual dispatch. Fork pushes use the native PX lane to avoid duplicate desktop,
PostgreSQL, and container test jobs on every server patch.

The native verification/deployment helpers also accept a root-only SSH package
via `package_path` in their private build specification, with SHA verification.
This avoids uploading build binaries through public inference endpoints.

## Compiled tool cache

In addition to Cargo dependencies and intermediate files, an exact-source cache
keeps the compiled `stravia-build-vendors` helper, Base manifest/Wasm and the
Codex/GA Wasm components. The key includes runner OS/architecture, pinned Rust,
lock files, Cargo configuration and all relevant component/SDK/codec sources.
A changed input creates a new cache; no broad fallback key reuses old binaries.
Restored files are verified against their recorded SHA256 and size before use.

The tool cache is saved immediately after successful compilation, before the
protocol/Core tests. If a later test fails, the compiled tools remain reusable,
while all tests still run on the next attempt. Deployable packages remain
separate, source-bound Actions artifacts; production data and credentials are
never cached. Builds queue behind an active run so a new push cannot discard
an expensive cold build before its cache has been saved.

## Platform plugin verification

The separate **Platform Plugins** workflow verifies the guest SDK and sandbox,
real component lifecycle and hooks, authenticated management API, and browser
management page. It publishes the independently installable example component.
It restores the shared Rust dependency cache without changing it, then uses a
separate cache keyed by backend sources, Cargo inputs and build icons to retain
workspace compilation results, including on a later test failure. This avoids
an existing dependency-cache hit preventing newly compiled workspace files from
being saved. Tests always run; Cargo still checks freshness after restoring
outputs, and changed source inputs select a new workspace cache.
The disposable Linux runner removes its unused Android and .NET toolchains
before building, leaving room for both compilation outputs and the compressed
cache archive. This cleanup never targets the repository or deployment hosts.

For a cache-only maintenance run, use **Run workflow → tools_only = true**.
It compiles or restores the helper and Wasm components, verifies their digests,
and saves a cache without rebuilding the unchanged server or emitting a release
package. A normal push or manual run with the option off performs all tests and
builds the deployable package. This also permits a second cache-only run to
verify that the exact compiled cache is reused.
