use serde_json::{Value, json};
use stravia_platform_plugin_sdk::{
    API_VERSION, HookKind, PlatformPlugin, PluginManifest, ToolManifest,
};

struct PolicyAndTools;

impl PlatformPlugin for PolicyAndTools {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: "example.policy-tools".into(),
            name: "Request policy and Echo tool".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            hooks: vec![
                HookKind::Request,
                HookKind::UpstreamResponse,
                HookKind::ToolResult,
                HookKind::ClientOutput,
            ],
            tools: vec![ToolManifest {
                name: "echo".into(),
                description: "Return the supplied JSON object unchanged".into(),
                parameters: json!({"type":"object","properties":{},"additionalProperties":true}),
                parallel_safe: true,
            }],
            config_schema: Some(json!({
                "type":"object",
                "properties":{
                    "system":{"type":"string","description":"Replace the system instruction"},
                    "block_model":{"type":"string","description":"Reject requests to this model"},
                    "response_text":{"type":"string","description":"Replace upstream response content"},
                    "client_text":{"type":"string","description":"Replace client output content"},
                    "tool_result":{"description":"Replace a platform tool result with this JSON value"}
                },
                "additionalProperties":false
            })),
        }
    }

    fn invoke(operation: &str, input: Value) -> Result<Value, String> {
        match operation {
            "request" => {
                let config = &input["config"];
                let mut actions = Vec::new();
                if config["block_model"]
                    .as_str()
                    .is_some_and(|blocked| input["request"]["model"].as_str() == Some(blocked))
                {
                    actions.push(json!({"type":"reject","status":403,"code":"model_blocked","message":"This model is blocked by request policy"}));
                } else if let Some(system) = config["system"].as_str() {
                    actions.push(json!({"type":"set_system","value":system}));
                }
                Ok(json!({"actions":actions}))
            }
            "upstream_response" | "client_output" => {
                let field = if operation == "upstream_response" {
                    "response_text"
                } else {
                    "client_text"
                };
                Ok(
                    json!({"actions":input["config"][field].as_str().map(|value| vec![json!({"type":"set_content","value":value})]).unwrap_or_default()}),
                )
            }
            "tool_result" => Ok(
                json!({"actions":input["config"].get("tool_result").map(|value| vec![json!({"type":"set_tool_result","value":value})]).unwrap_or_default()}),
            ),
            "tool:echo" => Ok(input.get("arguments").cloned().unwrap_or(Value::Null)),
            _ => Err(format!("unsupported operation: {operation}")),
        }
    }
}

stravia_platform_plugin_sdk::export_plugin!(PolicyAndTools);
