//! Pre-routing policy input is scoped by the host; no credential or principal
//! identifiers are exported. Returned routes are admitted by ordinary security.
use anyhow::{Context, ensure};
use serde::Deserialize;
use serde_json::json;

use super::{HookKind, PlatformPlugins};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resolution {
    target_model_id: Option<String>,
}

impl PlatformPlugins {
    pub(crate) async fn resolve_model(
        &self,
        requested: &str,
        key_mappings: &crate::request_models::RequestModelConfig,
        eligible_models: &[crate::request_models::RequestModelTarget],
    ) -> anyhow::Result<Option<Option<String>>> {
        let resolvers = self
            .snapshot()
            .into_iter()
            .filter(|plugin| {
                plugin
                    .loaded
                    .manifest()
                    .hooks
                    .contains(&HookKind::ResolveModel)
            })
            .collect::<Vec<_>>();
        ensure!(
            resolvers.len() <= 1,
            "Enable only one request model resolver"
        );
        let Some(plugin) = resolvers.first() else {
            return Ok(None);
        };
        let output = plugin
            .loaded
            .invoke(
                "resolve_model",
                &json!({
                    "event":"resolve_model", "config":plugin.config, "request_model":requested,
                    "key_mappings":key_mappings, "eligible_models":eligible_models,
                }),
            )
            .await
            .context("request model plugin failed")?;
        let result: Resolution = serde_json::from_value::<Resolution>(output)
            .context("invalid request model plugin result")?;
        if let Some(target) = &result.target_model_id {
            ensure!(
                eligible_models.iter().any(|model| model.id == *target),
                "request model plugin selected an unauthorized route"
            );
        }
        Ok(Some(result.target_model_id))
    }
}
