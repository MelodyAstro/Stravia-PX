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

For a cache-only maintenance run, use **Run workflow → tools_only = true**.
It compiles or restores the helper and Wasm components, verifies their digests,
and saves a cache without rebuilding the unchanged server or emitting a release
package. A normal push or manual run with the option off performs all tests and
builds the deployable package. This also permits a second cache-only run to
verify that the exact compiled cache is reused.

## Verified runs (2026-11-07)

- [Full native build](https://github.com/MelodyAstro/Stravia-PX/actions/runs/42367922887)
  passed protocol/Core checks, built the server and uploaded the private native
  package (`5fa4ad72e191245622b8b728cf0e6ea455ad3835`).
- [Compiled cache population](https://github.com/MelodyAstro/Stravia-PX/actions/runs/42373121365)
  built the tooling/components, recorded their digests, and saved the cache.
- [Exact cache reuse](https://github.com/MelodyAstro/Stravia-PX/actions/runs/42373946853)
  verified the restored binary digests and skipped both tool and managed Wasm
  compilation. No server rebuild or deployment was performed in cache-only mode.

The cache verification runs use commit `5965295e77d5887b2f6ec7df337ff8cee2c428b6`.
