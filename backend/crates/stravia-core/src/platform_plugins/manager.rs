use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, ensure};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use stravia_platform_plugin_runtime::{LoadedPlugin, PlatformPluginRuntime};
use tokio::sync::Mutex;

use super::{PluginManifest, PluginSnapshot, schema};

#[derive(Clone, Serialize, Deserialize)]
pub struct PlatformPluginSummary {
    pub manifest: PluginManifest,
    pub sha256: String,
    pub enabled: bool,
    pub config: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformPluginUpdate {
    pub enabled: Option<bool>,
    #[serde(default, deserialize_with = "present_configuration")]
    pub config: Option<Value>,
}

fn present_configuration<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

#[derive(Clone)]
struct Entry {
    summary: PlatformPluginSummary,
    loaded: Option<LoadedPlugin>,
}

pub(crate) struct PlatformPlugins {
    directory: PathBuf,
    runtime: PlatformPluginRuntime,
    entries: RwLock<BTreeMap<String, Entry>>,
    mutations: Mutex<()>,
}

impl PlatformPlugins {
    pub(crate) async fn open(data_dir: &Path) -> anyhow::Result<Arc<Self>> {
        let directory = data_dir.join("platform-plugins");
        tokio::fs::create_dir_all(directory.join("artifacts")).await?;
        let runtime = PlatformPluginRuntime::new()?;
        let records: Vec<PlatformPluginSummary> =
            match tokio::fs::read(directory.join("installed.json")).await {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .context("invalid platform plugin installation metadata")?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => {
                    return Err(error).context("reading platform plugin installation metadata");
                }
            };
        let mut entries = BTreeMap::new();
        for mut summary in records {
            ensure!(
                summary.sha256.len() == 64
                    && summary
                        .sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
                "invalid platform plugin artifact digest"
            );
            let id = summary.manifest.id.clone();
            ensure!(
                !entries.contains_key(&id),
                "duplicate platform plugin installation: {id}"
            );
            let loaded = async {
                let bytes = tokio::fs::read(
                    directory
                        .join("artifacts")
                        .join(format!("{}.wasm", summary.sha256)),
                )
                .await?;
                ensure!(
                    digest(&bytes) == summary.sha256,
                    "platform plugin artifact digest mismatch"
                );
                let loaded = runtime.load(bytes).await?;
                ensure!(
                    loaded.sha256() == summary.sha256,
                    "platform plugin artifact digest mismatch"
                );
                ensure!(
                    serde_json::to_value(loaded.manifest())?
                        == serde_json::to_value(&summary.manifest)?,
                    "platform plugin manifest mismatch"
                );
                validate_manifest(loaded.manifest())?;
                if summary.enabled {
                    schema::configuration(loaded.manifest(), &summary.config)?;
                }
                Ok::<_, anyhow::Error>(loaded)
            }
            .await;
            let loaded = match loaded {
                Ok(loaded) => {
                    summary.error = None;
                    Some(loaded)
                }
                Err(error) => {
                    tracing::warn!(plugin_id = %id, %error, "platform plugin could not be restored");
                    summary.error = Some(error.to_string());
                    None
                }
            };
            entries.insert(id, Entry { summary, loaded });
        }
        Ok(Arc::new(Self {
            directory,
            runtime,
            entries: RwLock::new(entries),
            mutations: Mutex::new(()),
        }))
    }

    pub(crate) fn list(&self) -> Vec<PlatformPluginSummary> {
        self.entries
            .read()
            .values()
            .map(|entry| entry.summary.clone())
            .collect()
    }

    pub(crate) fn snapshot(&self) -> Vec<PluginSnapshot> {
        self.entries
            .read()
            .values()
            .filter_map(|entry| {
                if !entry.summary.enabled {
                    return None;
                }
                Some(PluginSnapshot {
                    loaded: entry.loaded.clone()?,
                    config: entry.summary.config.clone(),
                })
            })
            .collect()
    }

    pub(crate) async fn install(
        self: &Arc<Self>,
        component: Vec<u8>,
    ) -> anyhow::Result<PlatformPluginSummary> {
        // Once admitted, a mutation owns its commit even if the HTTP caller
        // disconnects. Dropping its JoinHandle does not drop the writer lock.
        let manager = Arc::clone(self);
        tokio::spawn(async move { manager.install_inner(component, None).await })
            .await
            .context("platform plugin installation task failed")?
    }

    pub(crate) async fn replace(
        self: &Arc<Self>,
        id: &str,
        component: Vec<u8>,
    ) -> anyhow::Result<PlatformPluginSummary> {
        let manager = Arc::clone(self);
        let id = id.to_owned();
        tokio::spawn(async move { manager.install_inner(component, Some(id)).await })
            .await
            .context("platform plugin replacement task failed")?
    }

    async fn install_inner(
        &self,
        component: Vec<u8>,
        expected_id: Option<String>,
    ) -> anyhow::Result<PlatformPluginSummary> {
        // Validate and compile before taking the mutation lock or replacing an installation.
        let loaded = self.runtime.load(component.clone()).await?;
        validate_manifest(loaded.manifest())?;
        if let Some(expected_id) = &expected_id {
            ensure!(
                loaded.manifest().id == *expected_id,
                "plugin package identity does not match the selected plugin"
            );
        }
        let _mutation = self.mutations.lock().await;
        let mut next = self.entries.read().clone();
        if let Some(expected_id) = expected_id {
            ensure!(
                next.contains_key(&expected_id),
                "platform plugin is not installed"
            );
        }
        let manifest = loaded.manifest().clone();
        let config = next
            .get(&manifest.id)
            .map(|entry| entry.summary.config.clone())
            .unwrap_or_else(|| serde_json::json!({}));
        // An incompatible configuration may be repaired while the new version is disabled.
        let summary = PlatformPluginSummary {
            manifest,
            sha256: loaded.sha256().to_owned(),
            enabled: false,
            config,
            error: None,
        };
        let artifact = self
            .directory
            .join("artifacts")
            .join(format!("{}.wasm", summary.sha256));
        atomic_write(artifact, component).await?;
        next.insert(
            summary.manifest.id.clone(),
            Entry {
                summary: summary.clone(),
                loaded: Some(loaded),
            },
        );
        self.publish(next).await?;
        Ok(summary)
    }

    pub(crate) async fn update(
        self: &Arc<Self>,
        id: &str,
        update: PlatformPluginUpdate,
    ) -> anyhow::Result<PlatformPluginSummary> {
        let manager = Arc::clone(self);
        let id = id.to_owned();
        tokio::spawn(async move { manager.update_inner(&id, update).await })
            .await
            .context("platform plugin settings task failed")?
    }

    async fn update_inner(
        &self,
        id: &str,
        update: PlatformPluginUpdate,
    ) -> anyhow::Result<PlatformPluginSummary> {
        let _mutation = self.mutations.lock().await;
        let mut next = self.entries.read().clone();
        let entry = next
            .get_mut(id)
            .context("platform plugin is not installed")?;
        if let Some(config) = update.config {
            schema::configuration(&entry.summary.manifest, &config)?;
            entry.summary.config = config;
        }
        if let Some(enabled) = update.enabled {
            entry.summary.enabled = enabled;
        }
        if entry.summary.enabled {
            ensure!(
                entry.loaded.is_some(),
                "platform plugin is unavailable; install a valid component first"
            );
            schema::configuration(&entry.summary.manifest, &entry.summary.config)?;
        }
        let summary = entry.summary.clone();
        self.publish(next).await?;
        Ok(summary)
    }

    pub(crate) async fn uninstall(self: &Arc<Self>, id: &str) -> anyhow::Result<()> {
        let manager = Arc::clone(self);
        let id = id.to_owned();
        tokio::spawn(async move { manager.uninstall_inner(&id).await })
            .await
            .context("platform plugin uninstall task failed")?
    }

    async fn uninstall_inner(&self, id: &str) -> anyhow::Result<()> {
        let _mutation = self.mutations.lock().await;
        let mut next = self.entries.read().clone();
        ensure!(
            next.remove(id).is_some(),
            "platform plugin is not installed"
        );
        // Immutable artifacts remain available to already admitted requests.
        self.publish(next).await
    }

    async fn publish(&self, next: BTreeMap<String, Entry>) -> anyhow::Result<()> {
        let records: Vec<_> = next.values().map(|entry| &entry.summary).collect();
        atomic_write(
            self.directory.join("installed.json"),
            serde_json::to_vec_pretty(&records)?,
        )
        .await?;
        *self.entries.write() = next;
        Ok(())
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validate_manifest(manifest: &PluginManifest) -> anyhow::Result<()> {
    if let Some(schema) = &manifest.config_schema {
        schema::compile(schema)?;
    }
    for tool in &manifest.tools {
        schema::compile(&tool.parameters)?;
    }
    Ok(())
}

async fn atomic_write(path: PathBuf, bytes: Vec<u8>) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || {
        let parent = path.parent().context("plugin destination has no parent")?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&path).map_err(|error| error.error)?;
        #[cfg(unix)]
        if let Err(error) = std::fs::File::open(parent).and_then(|directory| directory.sync_all()) {
            // Rename is the commit point. Reporting a failed mutation here
            // would leave the published file and the in-memory state divergent.
            tracing::warn!(path = %parent.display(), %error, "platform plugin commit directory could not be synchronized");
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("writing platform plugin installation")?
}
