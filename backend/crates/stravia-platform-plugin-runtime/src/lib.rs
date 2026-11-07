//! Pure platform extensions run in fresh, bounded WebAssembly components.
//!
//! Only the unprivileged WASI 0.2 interfaces needed by Rust guests are admitted.
//! Filesystem and socket interfaces are rejected even if a guest declares them.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
pub use stravia_platform_plugin_sdk::{HookKind, PluginManifest, ToolManifest};
use stravia_platform_plugin_sdk::{MAX_MANIFEST_BYTES, MAX_MESSAGE_BYTES};
use tokio::sync::Semaphore;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

mod bindings {
    wasmtime::component::bindgen!({
        world: "platform-plugin",
        path: "../stravia-platform-plugin-sdk/wit",
        exports: { default: async },
    });
}

pub const MAX_COMPONENT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024;
pub const INVOCATION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_FUEL: u64 = 100_000_000;

#[derive(Clone)]
pub struct PlatformPluginRuntime {
    engine: Engine,
    compilation: Arc<Semaphore>,
}

#[derive(Clone)]
pub struct LoadedPlugin {
    inner: Arc<LoadedVersion>,
}

struct LoadedVersion {
    prepared: bindings::PlatformPluginPre<StoreState>,
    manifest: PluginManifest,
    sha256: String,
}

impl PlatformPluginRuntime {
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.concurrency_support(true);
        config.wasm_component_model_async(false);
        config.wasm_component_model_more_async_builtins(false);
        config.wasm_component_model_async_stackful(false);
        config.wasm_component_model_threading(false);
        config.consume_fuel(true);
        Ok(Self {
            engine: Engine::new(&config).context("create platform plugin engine")?,
            compilation: Arc::new(Semaphore::new(1)),
        })
    }

    pub async fn load(&self, bytes: Vec<u8>) -> Result<LoadedPlugin> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_COMPONENT_BYTES,
            "platform component must contain between 1 byte and 64 MiB"
        );
        let permit = tokio::time::timeout(
            Duration::from_secs(30),
            self.compilation.clone().acquire_owned(),
        )
        .await
        .context("platform compiler is busy")?
        .context("platform compiler closed")?;
        let engine = self.engine.clone();
        let compiled = tokio::task::spawn_blocking(move || -> Result<_> {
            // Keep the permit until compilation really finishes, including when
            // the caller cancels its wait. This bounds abandoned compilation.
            let _permit = permit;
            let sha256 = Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            let component =
                Component::from_binary(&engine, &bytes).context("compile platform component")?;
            validate_imports(&engine, &component)?;
            let mut linker = Linker::new(&engine);
            wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
            let prepared = bindings::PlatformPluginPre::new(linker.instantiate_pre(&component)?)
                .context("resolve platform plugin exports")?;
            Ok((prepared, sha256))
        });
        let (prepared, sha256) = tokio::time::timeout(Duration::from_secs(30), compiled)
            .await
            .context("platform component compilation timed out")??
            .context("load platform component")?;
        let manifest = read_manifest(&prepared).await?;
        // A manifest is immutable metadata, not an invocation-time permission
        // negotiation. Detect guests whose descriptor changes across instances.
        ensure!(
            manifest == read_manifest(&prepared).await?,
            "platform plugin manifest is not deterministic"
        );
        Ok(LoadedPlugin {
            inner: Arc::new(LoadedVersion {
                prepared,
                manifest,
                sha256,
            }),
        })
    }
}

impl LoadedPlugin {
    pub fn manifest(&self) -> &PluginManifest {
        &self.inner.manifest
    }
    pub fn sha256(&self) -> &str {
        &self.inner.sha256
    }

    pub async fn invoke(&self, operation: &str, input: &Value) -> Result<Value> {
        ensure!(
            self.manifest().supports(operation),
            "operation is not declared by the platform plugin"
        );
        let input = serde_json::to_string(input).context("encode platform plugin input")?;
        ensure!(
            input.len() <= MAX_MESSAGE_BYTES,
            "platform plugin input exceeds 4 MiB"
        );
        tokio::time::timeout(INVOCATION_TIMEOUT, async {
            let mut store = new_store(self.inner.prepared.engine())?;
            let instance = self
                .inner
                .prepared
                .instantiate_async(&mut store)
                .await
                .context("instantiate platform plugin")?;
            let output = instance
                .call_invoke(&mut store, operation, &input)
                .await
                .context("platform plugin trapped")?;
            let output = bounded_result(output, MAX_MESSAGE_BYTES)?;
            ensure!(
                output.len() <= MAX_MESSAGE_BYTES,
                "platform plugin output exceeds 4 MiB"
            );
            serde_json::from_str(&output).context("platform plugin returned invalid JSON")
        })
        .await
        .context("platform plugin invocation timed out")?
    }
}

async fn read_manifest(
    prepared: &bindings::PlatformPluginPre<StoreState>,
) -> Result<PluginManifest> {
    tokio::time::timeout(INVOCATION_TIMEOUT, async {
        let mut store = new_store(prepared.engine())?;
        let instance = prepared
            .instantiate_async(&mut store)
            .await
            .context("instantiate platform descriptor")?;
        let output = instance
            .call_describe(&mut store)
            .await
            .context("platform descriptor trapped")?;
        let output = bounded_result(output, MAX_MANIFEST_BYTES)?;
        ensure!(
            output.len() <= MAX_MANIFEST_BYTES,
            "platform plugin manifest exceeds 64 KiB"
        );
        let manifest: PluginManifest =
            serde_json::from_str(&output).context("invalid platform plugin manifest JSON")?;
        manifest.validate().map_err(anyhow::Error::msg)?;
        Ok(manifest)
    })
    .await
    .context("platform plugin descriptor timed out")?
}

fn bounded_result(output: Result<String, String>, maximum: usize) -> Result<String> {
    match output {
        Ok(value) => Ok(value),
        Err(error) => {
            ensure!(
                error.len() <= maximum,
                "platform plugin error exceeds message limit"
            );
            Err(anyhow::Error::msg(error))
        }
    }
}

fn validate_imports(engine: &Engine, component: &Component) -> Result<()> {
    for (name, _) in component.component_type().imports(engine) {
        let allowed = name.rsplit_once('@').is_some_and(|(interface, version)| {
            let release = version
                .split_once('+')
                .map_or(version, |(release, _)| release);
            release.strip_prefix("0.2.").is_some_and(|patch| {
                !patch.is_empty() && patch.bytes().all(|byte| byte.is_ascii_digit())
            }) && matches!(
                interface,
                "wasi:io/poll"
                    | "wasi:io/error"
                    | "wasi:io/streams"
                    | "wasi:clocks/monotonic-clock"
                    | "wasi:clocks/wall-clock"
                    | "wasi:random/random"
                    | "wasi:random/insecure"
                    | "wasi:random/insecure-seed"
                    | "wasi:cli/stdin"
                    | "wasi:cli/stdout"
                    | "wasi:cli/stderr"
                    | "wasi:cli/environment"
                    | "wasi:cli/exit"
                    | "wasi:cli/terminal-input"
                    | "wasi:cli/terminal-output"
                    | "wasi:cli/terminal-stdin"
                    | "wasi:cli/terminal-stdout"
                    | "wasi:cli/terminal-stderr"
            )
        });
        if !allowed {
            bail!("platform plugin imports a forbidden interface: {name}");
        }
    }
    Ok(())
}

struct StoreState {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
}

impl WasiView for StoreState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

fn new_store(engine: &Engine) -> Result<Store<StoreState>> {
    let mut wasi = WasiCtx::builder();
    // Defaults inherit nothing: no environment, command arguments, preopened
    // directories, sockets or standard streams. Random is bounded independently.
    wasi.max_random_size(64 * 1024);
    let limits = StoreLimitsBuilder::new()
        .memory_size(MAX_MEMORY_BYTES)
        .memories(1)
        .instances(16)
        .tables(4)
        .table_elements(10_000)
        .trap_on_grow_failure(true)
        .build();
    let mut store = Store::new(
        engine,
        StoreState {
            wasi: wasi.build(),
            table: ResourceTable::new(),
            limits,
        },
    );
    store.limiter(|state| &mut state.limits);
    store.set_fuel(MAX_FUEL)?;
    // A CPU-only guest yields so cancellation and the wall-clock timeout can
    // drop it, even if it never makes a host call.
    store.fuel_async_yield_interval(Some(100_000))?;
    Ok(store)
}

#[cfg(test)]
mod tests;
