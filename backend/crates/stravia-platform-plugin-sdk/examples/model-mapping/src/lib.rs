//! Request model policy, independently replaceable without rebuilding the host.
use serde_json::{Value, json};
use stravia_platform_plugin_sdk::{API_VERSION, HookKind, PlatformPlugin, PluginManifest};

struct ModelMapping;
impl PlatformPlugin for ModelMapping {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: "strinova.model-mapping".into(),
            name: "Request model mapping".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            hooks: vec![HookKind::ResolveModel],
            tools: Vec::new(),
            config_schema: Some(json!({"type":"object","properties":{
                "aliases":{"type":"object","additionalProperties":{"type":"string"},"description":"Client alias to an authorized public Route name"}
            },"additionalProperties":false})),
        }
    }
    fn invoke(operation: &str, input: Value) -> Result<Value, String> {
        if operation != "resolve_model" {
            return Err("unsupported operation".into());
        }
        let requested = input["request_model"]
            .as_str()
            .ok_or("missing request model")?;
        if input["key_mappings"]["enabled"].as_bool() != Some(true) {
            return Ok(json!({"target_model_id":null}));
        }
        // Explicit per-key mappings take precedence over plugin-wide aliases.
        let mapped = input["key_mappings"]["mappings"]
            .as_array()
            .and_then(|mappings| {
                mappings
                    .iter()
                    .find(|mapping| mapping["request_model"].as_str() == Some(requested))
            })
            .and_then(|mapping| mapping["target_model_id"].as_str());
        let alias = input["config"]["aliases"]
            .get(requested)
            .and_then(Value::as_str);
        let target = mapped.or_else(|| {
            alias.and_then(|name| {
                input["eligible_models"]
                    .as_array()?
                    .iter()
                    .find(|model| model["model_id"].as_str() == Some(name))?["id"]
                    .as_str()
            })
        });
        Ok(json!({"target_model_id":target}))
    }
}
stravia_platform_plugin_sdk::export_plugin!(ModelMapping);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_mapping_wins_and_other_keys_cannot_resolve_unavailable_routes() {
        let input = json!({"request_model":"gpt-current", "key_mappings":{"enabled":true,"mappings":[{"request_model":"gpt-current","target_model_id":"team-route"}]},"config":{"aliases":{"gpt-current":"private/gpt"}},"eligible_models":[{"id":"private-route","model_id":"private/gpt"}]});
        assert_eq!(
            ModelMapping::invoke("resolve_model", input.clone()).unwrap()["target_model_id"],
            "team-route"
        );
        let mut other = input;
        other["key_mappings"]["mappings"] = json!([]);
        other["eligible_models"] = json!([]);
        assert!(
            ModelMapping::invoke("resolve_model", other.clone()).unwrap()["target_model_id"]
                .is_null()
        );
        other["key_mappings"]["enabled"] = json!(false);
        assert!(ModelMapping::invoke("resolve_model", other).unwrap()["target_model_id"].is_null());
    }
}
