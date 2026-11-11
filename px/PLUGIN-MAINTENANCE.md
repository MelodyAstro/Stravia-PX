# Plugin-oriented maintenance

Strinova is based on StraviaPlatform v0.6.5. The stock upstream binary does not
provide Strinova's platform-component API. CI builds a matching patched host once;
compatible plugin replacements then need no host rebuild. Source patches are
not binary patches for arbitrary upstream executables.

The native artifact exports four independently checksummed source layers:

1. `platform-host.patch`: component runtime, scoped hooks, management and host
   authorization, ingress/codec and outbound transport capabilities.
2. `vendor-compatibility.patch`: independently compiled Claude/Codex/Antigravity
   provider policies, compatibility settings and identity generation.
3. `webui.patch`: lightweight branding and management UI changes.
4. `maintenance.patch`: CI, deployment and documentation.

`layers.json` pins the official base and fork source. The exporter verifies that
all layers reconstruct the complete source tree. Host contract changes and
ingress codec support still require the host patch; an ordinary post-decoding
request hook cannot implement them.

The **Strinova plugin components** workflow builds only provider components and
`strinova_model_mapping.wasm`, with compiled-tool and Cargo caches.

Install the mapping component in Platform Plugins and enable it. Exactly one
enabled `resolve_model` plugin is admitted. It receives this key's mapping rules
and eligible routes without key IDs or credentials. Explicit per-key mappings
take precedence. Optional configuration
`{"aliases":{"gpt-current":"team/gpt-6.1-sol"}}` adds an alias only when the
named route is eligible for the requesting key. The host validates every result;
ordinary routing/security checks remain active.

With no resolver enabled, existing per-key mappings retain their behavior.
Disabling/removing the resolver restores that behavior without rewriting saved
configuration. Internal route identities cannot be overridden. This is the first
extracted request policy; capture storage and admin UI remain host capabilities.

Request hooks receive `input_start`. New `splice_input` actions use suffix-relative
`start`, `end`, `items`. Legacy `replace_request` is translated into admitted
patches and cannot rewrite processed history or protected fields. Verify SDK/WIT
compatibility when upgrading the official host version.
