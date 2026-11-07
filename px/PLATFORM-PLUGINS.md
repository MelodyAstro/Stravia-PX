# Installable platform plugins

Platform plugins extend requests, responses and platform tools without rebuilding
the Stravia host. They are separate from Vendor plugins: a platform plugin does
not need to pretend to be a model supplier or provide authentication channels.
This host version must be installed once; subsequent compatible plugin changes
are independent `.wasm` uploads. No restart or host compilation is needed.

## Install and manage

Open **Platform Plugins** in the administration sidebar, choose a WebAssembly
Component, review its declared hooks and tools, edit its JSON configuration if
needed, then enable it. New installations and every component replacement are
disabled until explicitly enabled. Replacement preserves configuration, which
may need adjustment for the new version's schema. Invalid uploads preserve the
installed version and its enabled state.

The page supports component replacement, configuration, enable/disable and
confirmed uninstall. Configuration and tool arguments are validated against
the plugin's JSON Schemas locally. External schema references are rejected.
An unavailable or damaged component is listed with an error and cannot execute.

Changes affect newly admitted requests. A running inference or Agent run keeps
its original plugin versions and configuration, including its tools, until it
finishes. Disable and uninstall stop admission of new work; already admitted
work continues. An administration request that disconnects after a mutation
starts does not abort the commit: reload the list to check its result.

## Build a plugin

The SDK and versioned WIT are in
[`stravia-platform-plugin-sdk`](../backend/crates/stravia-platform-plugin-sdk).
The SDK exports `describe` and `invoke` through the Component Model, with UTF-8
JSON payloads. It does not expose Rust host traits or database objects.

The working example implements request policy, response and tool result
transforms, and an Echo tool:

```sh
task build:platform-plugin-example
```

Install `target/wasm32-wasip2/release/stravia_platform_plugin_example.wasm`.
For example, configure `{"system":"Use concise answers."}` before enabling it.
Other optional example settings are `block_model`, `response_text`,
`client_text` and `tool_result`. The example demonstrates all four hook stages;
omit response hooks in a request-only production plugin to preserve live
response streaming.

Use the example's `cdylib` crate as a starting point, implement
`PlatformPlugin::manifest` and `PlatformPlugin::invoke`, and call
`stravia_platform_plugin_sdk::export_plugin!(YourPlugin)`. Build the guest for
`wasm32-wasip2`. Only that plugin needs compiling when its implementation changes.
The SDK is kept in this repository rather than published to crates.io.

The manifest is immutable metadata and includes:

| Field | Meaning |
| --- | --- |
| `id`, `name`, `version` | Stable unique identity, display name and semantic version |
| `api_version` | JSON protocol version; currently `1` |
| `hooks` | Any of `request`, `upstream_response`, `tool_result`, `client_output` |
| `tools` | Named tools with description, object JSON Schema `parameters`, and optional `parallel_safe` |
| `config_schema` | Optional JSON Schema for the administrator's configuration object |

The host validates metadata, contract compatibility and SHA256 before admitting
the component. A descriptor must be consistent across fresh instances. Uploads
with the same `id` replace that installation; use a different identity for a
separate plugin. The row's **Update from file** action additionally verifies
the uploaded identity matches that installed row before changing anything.

## Hook protocol

`invoke(operation, input)` receives an operation matching a declared hook:

| Operation | Input fields in addition to `config`, `event`, `round` |
| --- | --- |
| `request` | `request`, and `route` when available |
| `upstream_response` | `request`, `response`, `route` |
| `tool_result` | `result` containing tool/call identifiers, content, error flag and metadata; `route` |
| `client_output` | `response`, `route` |

Requests and responses use Stravia's canonical JSON representation. A route
contains model/target identifiers and egress protocol when known. Provider
connection records, authorization headers, API key principals and host
credentials are excluded. Canonical business content can still contain user
data; an enabled hook is authorized to process that content.

A hook returns an object with an `actions` array. Return `{"actions":[]}` to
continue without changing the event.

```json
{"actions":[{"type":"set_system","value":"Answer concisely."}]}
```

Supported actions reuse the host's existing hook validation and ordering:

| Action | Fields | Stage |
| --- | --- | --- |
| `set_system` | `value`: string or null | Request |
| `set_model` | `value`: string | Request |
| `replace_request` | `value`: canonical request | Request |
| `set_content` | `value`: string | Upstream response / client output |
| `set_reasoning` | `value`: string or null | Upstream response / client output |
| `replace_response` | `value`: canonical response | Upstream response / client output |
| `set_tool_result` | `value`: JSON | Tool result |
| `reject` | HTTP `status` 400–599, nonempty `code`, `message` | Request |

Plugins run after built-in hooks, in stable plugin identity order. Actions are
applied sequentially, so later plugins see earlier changes. Invalid actions,
guest errors, traps and timeouts fail through the existing hook error path;
they are not silently ignored. At most 64 actions may be returned per event.

Response hooks process the completed canonical response. Subscribing to
`upstream_response` or `client_output` selects the existing terminal buffering
path for SSE and WebSocket responses, increasing time to first output. Request
and tool-only plugins do not select this buffering path. Per-token stream
mutation and new host capabilities require a future contract/host change.

## Tools, Agent runs and MCP

A tool invocation uses `operation = "tool:<manifest name>"` and receives
`{"config":{...},"arguments":{...}}`. Return the business JSON result directly;
return a WIT error for failure. The host validates arguments before calling the
guest. Guest state starts fresh on every invocation; configuration is the
explicit source of retained settings.

Enabled plugin tools are exposed to ordinary generation requests. Fixed Agent
tool allowlists remain authoritative; plugin tools can be included explicitly.
An Agent definition references `{ "id": "platform-plugin:<plugin-id>:<tool-name>",
"version": 1 }` in its tool allowlist.
MCP `tools/list` and `tools/call` discover currently enabled tools, with the
same authentication and visibility path as existing platform tools. Public
MCP names have the form `plugin_<readable-name>_<identity-hash>`; inference tool
names additionally use the existing `stravia__` prefix. These identities avoid
collisions with built-ins and other installations. Discover the actual name through MCP
or the existing tool registry instead of constructing it by hand.

An Agent run pins one plugin snapshot for its entire execution. An MCP call
pins the current version when the call starts; a preceding `tools/list` does
not reserve a version. If a plugin changes between listing and calling,
validation uses the current schema. Refresh the tool list after a change.

## Isolation and persistence

The runtime uses Wasmtime Component Model and versioned WIT, consistent with
the Vendor runtime's architecture. It admits only a narrow set of unprivileged
WASI interfaces needed by Rust guests. There are no filesystem or socket
imports, inherited environment, host arguments, standard streams, preopened
directories, database handles or provider credentials.

Limits per invocation are 64 MiB linear memory, 100 million fuel units and a
five-second wall-clock deadline with periodic cooperative yields. Inputs and
outputs are limited to 4 MiB of JSON, the manifest to 64 KiB, and the uploaded
component to 64 MiB. Compilation is serialized per runtime. These constraints
are suitable for policies and pure tools; network-backed Provider integrations
continue to use Vendor plugins and their explicit network capabilities.

Install state is under `<data_dir>/platform-plugins/installed.json`, with
immutable SHA256-addressed artifacts in `artifacts/`. Writes use a synced
temporary file and atomic replacement before publishing a new memory snapshot.
Include this entire directory in backups. Retained artifacts support running
snapshots and manual recovery; uninstall removes the active installation and
does not erase historical artifacts.

Administration state belongs to one Stravia process/data directory. Separate
instances, including hosts sharing a SQL database, need their own plugin
installation and directory synchronization. Concurrent processes must not
manage the same plugin directory.

## HTTP API and verification

All endpoints use the existing authenticated administration API, including its
CSRF checks for browser writes, and return `Cache-Control: no-store`.

| Method and path | Body / result |
| --- | --- |
| `GET /api/v1/platform-plugins` | Installed summaries |
| `POST /api/v1/platform-plugins` | Raw `application/wasm`; installs/replaces and returns a disabled summary |
| `POST /api/v1/platform-plugins/{id}` | Raw `application/wasm`; replaces only the matching installed identity |
| `PUT /api/v1/platform-plugins/{id}/settings` | Optional `enabled` and `config` fields; commits both atomically |
| `DELETE /api/v1/platform-plugins/{id}` | Uninstalls the active entry |

Successful responses are wrapped in `{"data":...}`; errors use `{"error":...}`.
A summary contains `manifest`, `sha256`, `enabled`, `config`, and optional `error`.

Run `task test:platform-plugins` for the real component's sandbox, lifecycle,
snapshot, hook/tool and authenticated HTTP regressions. The **Platform Plugins**
GitHub workflow runs these checks and the management page's browser tests on
GitHub-hosted Linux, then publishes the independent example component with its
source commit and SHA256. It requires no production credentials and does not
deploy. The existing **PX Native Linux** workflow builds the updated host.
