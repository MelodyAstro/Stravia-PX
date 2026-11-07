# Deploying the platform-plugin host

Use the successful **PX Native Linux** package for the exact reviewed commit.
The separate **Platform Plugins** workflow validates the real guest and its
management paths; it does not deploy. This runbook describes the upgrade
procedure, not evidence that a production upgrade has already happened.

`ops/deploy-platform-plugins.py` replaces a native build on the same upstream
version. It uses the existing Alpine/OpenRC service, independent GNU runtime,
management helper and complete snapshot/restore tool. It does not change Key
permissions, routing, model aliases, provider options, Debug or Nginx settings.

Prepare a root-only specification on the deployment host:

```json
{
  "source_commit": "<full reviewed commit>",
  "package_path": "/root/stravia-px-native.tar.gz",
  "package_sha256": "<SHA256 of the native tar.gz>",
  "build_manifest_sha256": "<SHA256 of build-manifest.json>",
  "previous_host_sha256": "<SHA256 of the currently running executable>",
  "release_name": "masub-platform-plugins-1",
  "vendor_versions": {
    "base": "<version declared by the packaged Base component>",
    "openai-codex": "<version declared by the packaged Codex component>",
    "antigravity": "<version declared by the packaged GA component>"
  },
  "required_features": [
    "positional_additional_tools",
    "per_key_request_model_mapping_v1",
    "native_custom_tools_v1",
    "platform_plugins_v1"
  ]
}
```

Never copy the sample placeholders as values. Obtain package/manifest hashes
from the verified Actions artifact and read the previous executable digest
from the actual deployment. Vendor versions must match the built components,
including local suffixes; the upstream release tag alone is insufficient.

```sh
python3 px/ops/deploy-platform-plugins.py --spec /root/stravia-platform-plugins-build.json --dry-run
python3 px/ops/deploy-platform-plugins.py --spec /root/stravia-platform-plugins-build.json
```

The read-only run validates source and package pins, the complete manifest,
safe archive members, the existing program/managed-component digests, a new
release directory and immutable resource collisions. It prints a disk budget
for the executable/components, new assets, gzip companions, backup, rollback
extraction and free
space margin. It does not start programs, call the API, create directories or
stop the service. The real run repeats these checks and rejects insufficient
space before staging.

By default the backup reserve uses the uncompressed size of the data root and
active compatibility trees, plus archive overhead. If a verified compressed
backup estimate is needed on a small disk, set `backup_reserve_bytes` explicitly
to that estimate plus its growth allowance. `free_space_margin_bytes` defaults
to 128 MiB and cannot be below 32 MiB. Include space for the retained old
program and backup; the script never deletes them to make room.

The separate `rollback_reserve_bytes` budget is always calculated from the
current data root's uncompressed file sizes, plus directory/write overhead.
Restore retains the newer data tree before extracting the old one, so a second
unpacked data tree must fit in addition to the compressed backup. Lowering the
backup estimate does not lower this rollback reserve.

The package must include `stravia-server`, `base.wasm`, `codex.wasm`,
`antigravity.wasm`, `source.patch`, `build-manifest.json` and `immutable/`.
`policy-tools.wasm` is optional and is staged without being enabled. Every
flat file must have a digest and size in the build manifest, except the
manifest itself, which has its separate specification pin. The manifest must
declare `platform_plugins_v1` and retain every active managed feature. Its
source patch must include the root Cargo manifests/lock file and other build
inputs needed to reconstruct the recorded commit.

The script preloads immutable files and gzip companions, refusing existing
same-name files with different contents. It streams the executable to disk
to limit staging memory. Existing resources remain for old pages and rollback.
It checks the candidate through the isolated GNU loader's `--list` and
`--version`, then runs `stravia-maint backup` to obtain a stopped-service
snapshot before switching the current-release symlink.

After restart it checks readiness, the exact Vendor inventory and component
digests, provider credential status and compatibility options, Key/route/mapping fingerprints,
SQLite integrity, platform-plugin administration, Debug and Nginx hashes.
Codex/GA components are reimported only if their verified bytes differ from
the installed digest; confirmation never permits data discard. Base comes
from the host's embedded component. Unrelated Vendors retain their versions
and digests. Active Responses maintenance metadata receives the new source,
host, Base, component and feature pins. GA identity/provider metadata is
preserved; its source/digest metadata changes only when its component changes.

A failure after cutover starts invokes `stravia-maint restore` using this
deployment's complete snapshot. This restores data and managed metadata with
the matching old program; newer data is retained separately by the maintenance
helper. Staged programs and additional immutable resources remain available
for diagnosis. Errors during preflight do not initiate rollback. Choose a new
`release_name` for another attempt rather than overwriting a staged directory.
Run one maintenance operation at a time.

On success a root-only report is written to
`/root/stravia-platform-plugins-deployment.json`; `report_path` can override it.
The report contains hashes, paths and verification results, not credentials.
It does not claim a successful public inference or browser test. Follow with
both domains' login/new static resources and complete HTTP/SSE/WebSocket tool
round trips using the existing verification helpers.

The complete data snapshot recursively includes
`/var/lib/stravia/platform-plugins/installed.json` and its artifact directory.
After the first plugin installation, inspect a fresh backup's members to
confirm those files and keep the verified native package with the backup.
See [platform plugin behavior and API](PLATFORM-PLUGINS.md) for hot-install
acceptance tests. The example declares response hooks and selects completed
response buffering when enabled; use an isolated instance or a deliberate
short acceptance window for its full execution test.

Run the deployment package regressions without a server or credentials:

```sh
python3 px/ops/test-deploy-platform-plugins.py
```
