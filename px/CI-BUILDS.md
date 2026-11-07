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
