# Inject/plugin installation

Public releases publish the Linux x86-64 server package and signed Wasm plugin
artifacts. Install a plugin through the server's platform-plugin management
API or the WebUI, verify the release digest and signature, then enable it for
the intended platform scope.

Do not copy vendor source into an application checkout. Pin the artifact
version and digest in deployment automation, and keep the private patch token
out of runtime configuration.
