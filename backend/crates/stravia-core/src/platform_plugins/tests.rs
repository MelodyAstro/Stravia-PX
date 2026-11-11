use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use stravia_runtime_contract::Principal;
use stravia_runtime_contract::hook::*;
use stravia_runtime_contract::protocol::ir::{AiRequest, AiResponse, ToolResultContentKind};

use super::*;

fn fixture() -> Vec<u8> {
    let path = std::env::var_os("STRAVIA_PLATFORM_PLUGIN_FIXTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../target/wasm32-wasip2/release/stravia_platform_plugin_example.wasm")
        });
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "build the real plugin fixture with task build:platform-plugin-example ({}): {error}",
            path.display()
        )
    })
}

fn context() -> SessionContext {
    SessionContext {
        tools_fixed: false,
        request_id: "plugin-request".into(),
        run_id: "plugin-run".into(),
        request_kind: RequestKind::Generation,
        ingress: stravia_runtime_contract::protocol::ids::OPENAI_COMPATIBLE_CHAT_COMPLETIONS_V1,
        transport: TransportKind::Http,
        principal: Principal::new("plugin-test-key"),
        cancellation: Default::default(),
        inherited_media_turns: Vec::new(),
        response_id: None,
        previous_response_id: None,
    }
}

#[tokio::test]
async fn model_policy_component_is_independent_scoped_and_can_be_disabled() {
    let directory = tempfile::tempdir().unwrap();
    let manager = PlatformPlugins::open(directory.path()).await.unwrap();
    let component = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/wasm32-wasip2/release/strinova_model_mapping.wasm");
    let summary = manager
        .install(std::fs::read(component).unwrap())
        .await
        .unwrap();
    let config = crate::request_models::RequestModelConfig {
        enabled: true,
        mappings: Vec::new(),
    };
    let eligible = vec![crate::request_models::RequestModelTarget {
        id: "team".into(),
        model_id: "team/gpt".into(),
        display_name: "Team".into(),
    }];
    assert!(
        manager
            .resolve_model("gpt", &config, &eligible)
            .await
            .unwrap()
            .is_none()
    );
    manager
        .update(
            &summary.manifest.id,
            PlatformPluginUpdate {
                enabled: Some(true),
                config: Some(json!({"aliases":{"gpt":"team/gpt"}})),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        manager
            .resolve_model("gpt", &config, &eligible)
            .await
            .unwrap(),
        Some(Some("team".into()))
    );
    assert_eq!(
        manager.resolve_model("gpt", &config, &[]).await.unwrap(),
        Some(None)
    );
    manager
        .update(
            &summary.manifest.id,
            PlatformPluginUpdate {
                enabled: Some(false),
                config: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        manager
            .resolve_model("gpt", &config, &eligible)
            .await
            .unwrap(),
        None
    );
}

async fn tools_gateway(directory: &Path) -> crate::Gateway {
    crate::Gateway::new(crate::config::GatewayConfig {
        data_dir: directory.to_path_buf(),
        ..Default::default()
    })
    .await
    .unwrap()
}

async fn tools_key(gateway: &crate::Gateway, mcp_access: bool) -> String {
    gateway
        .admin()
        .create_api_key(
            serde_json::from_value(json!({
                "name":"Plugin tool principal",
                "mcp_access_enabled":mcp_access,
                "transparent_injection_enabled":false
            }))
            .unwrap(),
        )
        .await
        .unwrap()
        .id
}

async fn update_tools_key(gateway: &crate::Gateway, id: &str, input: serde_json::Value) {
    gateway
        .admin()
        .update_api_key(id, serde_json::from_value(input).unwrap())
        .await
        .unwrap();
}

async fn enable_fixture(manager: &Arc<PlatformPlugins>) {
    let installed = manager.install(fixture()).await.unwrap();
    manager
        .update(
            &installed.manifest.id,
            PlatformPluginUpdate {
                enabled: Some(true),
                config: None,
            },
        )
        .await
        .unwrap();
}

#[test]
fn configuration_schemas_reject_remote_references_and_enforce_values() {
    assert!(
        super::schema::compile(&json!({"$ref":"https://example.invalid/schema.json"})).is_err()
    );
    assert!(
        super::schema::compile(&json!({"$defs":{"nested":{"$dynamicRef":"file:///etc/passwd"}}}))
            .is_err()
    );
    let schema = json!({"type":"object","properties":{"mode":{"enum":["safe"]}},"required":["mode"],"additionalProperties":false});
    assert!(super::schema::validate(&schema, &json!({"mode":"safe"})).is_ok());
    assert!(super::schema::validate(&schema, &json!({"mode":"unsafe"})).is_err());
    assert!(super::schema::validate(&schema, &json!({})).is_err());
}

#[tokio::test]
async fn install_settings_restart_and_failed_update_preserve_the_committed_plugin() {
    let directory = tempfile::tempdir().unwrap();
    let manager = PlatformPlugins::open(directory.path()).await.unwrap();
    let bytes = fixture();
    let installed = manager.install(bytes.clone()).await.unwrap();
    let id = installed.manifest.id.as_str();
    assert!(!installed.enabled);
    assert!(manager.snapshot().is_empty());
    manager
        .update(
            id,
            PlatformPluginUpdate {
                enabled: Some(true),
                config: Some(json!({"system":"first"})),
            },
        )
        .await
        .unwrap();
    assert_eq!(manager.snapshot().len(), 1);
    assert!(
        manager
            .update(
                id,
                PlatformPluginUpdate {
                    enabled: None,
                    config: Some(json!({"system":123}))
                }
            )
            .await
            .is_err()
    );
    assert!(
        manager
            .install(b"not a WebAssembly component".to_vec())
            .await
            .is_err()
    );
    assert_eq!(manager.list()[0].config, json!({"system":"first"}));
    assert!(manager.list()[0].enabled);

    let restored = PlatformPlugins::open(directory.path()).await.unwrap();
    assert_eq!(restored.list()[0].sha256, installed.sha256);
    assert_eq!(restored.snapshot().len(), 1);
    let updated = manager.install(bytes).await.unwrap();
    assert!(
        !updated.enabled,
        "every new component requires an explicit enable action"
    );
    assert_eq!(updated.config, json!({"system":"first"}));
    manager.uninstall(id).await.unwrap();
    assert!(manager.list().is_empty());
    assert!(
        PlatformPlugins::open(directory.path())
            .await
            .unwrap()
            .list()
            .is_empty()
    );
}

#[tokio::test]
async fn replacement_requires_the_installed_identity_and_preserves_settings_on_rejection() {
    let directory = tempfile::tempdir().unwrap();
    let manager = PlatformPlugins::open(directory.path()).await.unwrap();
    let bytes = fixture();
    let expected_id = "example.policy-tools";
    assert!(manager.replace(expected_id, bytes.clone()).await.is_err());
    assert!(manager.list().is_empty());
    assert!(
        !directory
            .path()
            .join("platform-plugins/installed.json")
            .exists()
    );
    assert_eq!(
        std::fs::read_dir(directory.path().join("platform-plugins/artifacts"))
            .unwrap()
            .count(),
        0
    );

    let installed = manager.install(bytes.clone()).await.unwrap();
    assert_eq!(installed.manifest.id, expected_id);
    manager
        .update(
            expected_id,
            PlatformPluginUpdate {
                enabled: Some(true),
                config: Some(json!({"system":"keep this policy"})),
            },
        )
        .await
        .unwrap();
    let metadata_path = directory.path().join("platform-plugins/installed.json");
    let before = std::fs::read(&metadata_path).unwrap();
    let rejected = manager
        .replace("another.plugin", bytes.clone())
        .await
        .err()
        .expect("a different package identity must be rejected");
    assert!(rejected.to_string().contains("identity"));
    let current = manager.list();
    assert_eq!(current.len(), 1);
    assert!(current[0].enabled);
    assert_eq!(current[0].config, json!({"system":"keep this policy"}));
    assert_eq!(current[0].sha256, installed.sha256);
    assert_eq!(std::fs::read(&metadata_path).unwrap(), before);
    assert_eq!(manager.snapshot().len(), 1);

    let replaced = manager.replace(expected_id, bytes.clone()).await.unwrap();
    assert!(!replaced.enabled);
    assert_eq!(replaced.config, json!({"system":"keep this policy"}));
    manager.uninstall(expected_id).await.unwrap();
    assert!(manager.replace(expected_id, bytes).await.is_err());
    assert!(manager.list().is_empty());
}

#[tokio::test]
async fn explicit_null_configuration_is_rejected_without_changing_committed_settings() {
    let directory = tempfile::tempdir().unwrap();
    let manager = PlatformPlugins::open(directory.path()).await.unwrap();
    let installed = manager.install(fixture()).await.unwrap();
    let id = &installed.manifest.id;
    manager
        .update(
            id,
            PlatformPluginUpdate {
                enabled: Some(true),
                config: Some(json!({"system":"keep this policy"})),
            },
        )
        .await
        .unwrap();
    let metadata_path = directory.path().join("platform-plugins/installed.json");
    let before = std::fs::read(&metadata_path).unwrap();
    let input: PlatformPluginUpdate =
        serde_json::from_value(json!({"enabled":false,"config":null})).unwrap();
    assert_eq!(input.config, Some(serde_json::Value::Null));
    assert!(manager.update(id, input).await.is_err());
    let current = manager.list();
    assert!(current[0].enabled);
    assert_eq!(current[0].config, json!({"system":"keep this policy"}));
    assert_eq!(std::fs::read(&metadata_path).unwrap(), before);
    let omitted: PlatformPluginUpdate = serde_json::from_value(json!({"enabled":false})).unwrap();
    assert_eq!(omitted.config, None);
    manager.update(id, omitted).await.unwrap();
    assert!(!manager.list()[0].enabled);
    assert_eq!(
        manager.list()[0].config,
        json!({"system":"keep this policy"})
    );
}

#[tokio::test]
async fn hook_and_tool_requests_pin_config_and_survive_disable_and_uninstall() {
    let directory = tempfile::tempdir().unwrap();
    let manager = PlatformPlugins::open(directory.path()).await.unwrap();
    let installed = manager.install(fixture()).await.unwrap();
    let id = &installed.manifest.id;
    manager
        .update(
            id,
            PlatformPluginUpdate {
                enabled: Some(true),
                config: Some(json!({"system":"old policy"})),
            },
        )
        .await
        .unwrap();
    let runtime = crate::hook::HookRuntime::default().with_platform_plugins(Arc::clone(&manager));
    let mut old_request = AiRequest::new("model", Vec::new());
    let mut old_run = runtime
        .begin(context(), &old_request, ContextCompleteness::Full, 0)
        .unwrap();
    let old_tools = snapshot_tools(&manager.snapshot());
    manager
        .update(
            id,
            PlatformPluginUpdate {
                enabled: None,
                config: Some(json!({"system":"new policy"})),
            },
        )
        .await
        .unwrap();
    let mut new_request = AiRequest::new("model", Vec::new());
    let mut new_run = runtime
        .begin(context(), &new_request, ContextCompleteness::Full, 0)
        .unwrap();
    manager
        .update(
            id,
            PlatformPluginUpdate {
                enabled: Some(false),
                config: None,
            },
        )
        .await
        .unwrap();
    let mut disabled_request = AiRequest::new("model", Vec::new());
    let mut disabled_run = runtime
        .begin(context(), &disabled_request, ContextCompleteness::Full, 0)
        .unwrap();
    manager.uninstall(id).await.unwrap();

    assert!(matches!(
        old_run
            .on_request_with_provenance(&mut old_request)
            .await
            .unwrap()
            .control,
        HookControl::Continue
    ));
    assert_eq!(old_request.instructions.as_deref(), Some("old policy"));
    assert!(!old_request.tools.as_ref().unwrap().is_empty());
    new_run
        .on_request_with_provenance(&mut new_request)
        .await
        .unwrap();
    assert_eq!(new_request.instructions.as_deref(), Some("new policy"));
    disabled_run
        .on_request_with_provenance(&mut disabled_request)
        .await
        .unwrap();
    assert_eq!(disabled_request.instructions, None);
    assert!(disabled_request.tools.is_none());
    let output = old_tools[0]
        .execute(
            json!({"echo":42}),
            ToolExecutionContext {
                request_id: "req".into(),
                run_id: "run".into(),
                principal: Principal::new("plugin-test-key"),
                read_scope: ReadExposureScope::NONE,
                cancellation: Default::default(),
                progress: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(output, json!({"echo":42}));
}

#[tokio::test]
async fn disk_failure_does_not_publish_configuration_changes() {
    let directory = tempfile::tempdir().unwrap();
    let manager = PlatformPlugins::open(directory.path()).await.unwrap();
    let installed = manager.install(fixture()).await.unwrap();
    let metadata = directory.path().join("platform-plugins/installed.json");
    std::fs::remove_file(&metadata).unwrap();
    std::fs::create_dir(&metadata).unwrap();
    assert!(
        manager
            .update(
                &installed.manifest.id,
                PlatformPluginUpdate {
                    enabled: Some(true),
                    config: Some(json!({"system":"uncommitted"}))
                }
            )
            .await
            .is_err()
    );
    assert!(!manager.list()[0].enabled);
    assert_eq!(manager.list()[0].config, json!({}));
}

#[tokio::test]
async fn corrupt_artifacts_are_reported_and_excluded_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let manager = PlatformPlugins::open(directory.path()).await.unwrap();
    let installed = manager.install(fixture()).await.unwrap();
    manager
        .update(
            &installed.manifest.id,
            PlatformPluginUpdate {
                enabled: Some(true),
                config: None,
            },
        )
        .await
        .unwrap();
    std::fs::write(
        directory.path().join(format!(
            "platform-plugins/artifacts/{}.wasm",
            installed.sha256
        )),
        b"corrupt",
    )
    .unwrap();
    let restored = PlatformPlugins::open(directory.path()).await.unwrap();
    assert!(restored.list()[0].error.is_some());
    assert!(restored.snapshot().is_empty());
    assert_eq!(
        manager.snapshot().len(),
        1,
        "already loaded snapshots retain their verified component"
    );
}

#[tokio::test]
async fn all_hook_stages_apply_through_the_existing_contract() {
    let directory = tempfile::tempdir().unwrap();
    let manager = PlatformPlugins::open(directory.path()).await.unwrap();
    let installed = manager.install(fixture()).await.unwrap();
    manager.update(&installed.manifest.id, PlatformPluginUpdate { enabled:Some(true), config:Some(json!({"response_text":"rewritten upstream", "client_text":"rewritten client", "tool_result":{"rewritten":true}})) }).await.unwrap();
    let runtime = crate::hook::HookRuntime::default().with_platform_plugins(manager);
    let mut request = AiRequest::new("model", Vec::new());
    let mut run = runtime
        .begin(context(), &request, ContextCompleteness::Full, 0)
        .unwrap();
    assert!(run.requires_terminal_buffering());
    run.on_request_with_provenance(&mut request).await.unwrap();
    run.set_route(RouteContext {
        model_id: "route".into(),
        provider_id: "private-provider".into(),
        target_id: "target".into(),
        egress: None,
    });
    let mut response = AiResponse::new("response-id", "model");
    response.push_output_text("original output");
    run.on_upstream_response(&request, &mut response)
        .await
        .unwrap();
    assert_eq!(response.output_text(), "rewritten upstream");
    assert_eq!(response.id, "response-id");
    assert_eq!(response.model, "model");
    let mut result = PlatformToolResult {
        tool_id: ToolId::new("test-tool"),
        call_id: "call".into(),
        content: json!({"original":true}),
        content_kind: ToolResultContentKind::Json,
        is_error: false,
        metadata: Default::default(),
    };
    run.on_tool_result(&mut result).await.unwrap();
    assert_eq!(result.content, json!({"rewritten":true}));
    assert_eq!(result.call_id, "call");
    run.on_client_output(&mut response).await.unwrap();
    assert_eq!(response.output_text(), "rewritten client");
}

#[tokio::test]
async fn mcp_inventory_changes_without_rebuilding_and_accepted_calls_remain_pinned() {
    let directory = tempfile::tempdir().unwrap();
    let gateway = tools_gateway(directory.path()).await;
    let manager = Arc::clone(&gateway.platform_plugins);
    let registry = gateway.mcp_registry.clone();
    let key_id = tools_key(&gateway, true).await;
    let installed = manager.install(fixture()).await.unwrap();
    manager
        .update(
            &installed.manifest.id,
            PlatformPluginUpdate {
                enabled: Some(true),
                config: None,
            },
        )
        .await
        .unwrap();
    let tools = snapshot_tools(&manager.snapshot());
    let name = tools[0].external_name();
    let accepted = registry
        .resolve(name)
        .expect("hot-installed tool is immediately discoverable");
    manager
        .update(
            &installed.manifest.id,
            PlatformPluginUpdate {
                enabled: Some(false),
                config: None,
            },
        )
        .await
        .unwrap();
    assert!(registry.resolve(name).is_none());
    let output = accepted
        .call(json!({"value":7}), &crate::mcp::McpContext::new(key_id))
        .await
        .unwrap();
    assert_eq!(output.structured_content, json!({"result":{"value":7}}));
    assert!(!output.is_error);
    manager
        .update(
            &installed.manifest.id,
            PlatformPluginUpdate {
                enabled: Some(true),
                config: None,
            },
        )
        .await
        .unwrap();
    assert!(registry.resolve(name).is_some());
    manager.uninstall(&installed.manifest.id).await.unwrap();
    assert!(registry.resolve(name).is_none());
}

#[tokio::test]
async fn mcp_plugin_tool_handles_recheck_live_key_permissions() {
    let directory = tempfile::tempdir().unwrap();
    let gateway = tools_gateway(directory.path()).await;
    enable_fixture(&gateway.platform_plugins).await;
    let key_id = tools_key(&gateway, false).await;
    let context = crate::mcp::McpContext::new(key_id.clone());
    let tool = mcp_tools(&gateway.platform_plugins.snapshot(), &gateway.storage).remove(0);
    assert!(!tool.available(&context).await.unwrap());
    assert_eq!(
        tool.call(json!({"value":7}), &context)
            .await
            .unwrap_err()
            .code,
        "tool_unavailable"
    );

    update_tools_key(&gateway, &key_id, json!({"mcp_access_enabled":true})).await;
    assert!(tool.available(&context).await.unwrap());
    assert_eq!(
        tool.call(json!({"value":7}), &context)
            .await
            .unwrap()
            .structured_content,
        json!({"result":{"value":7}})
    );

    // A handle obtained while access was allowed cannot bypass later revocation.
    update_tools_key(&gateway, &key_id, json!({"mcp_access_enabled":false})).await;
    assert_eq!(
        tool.call(json!({"value":7}), &context)
            .await
            .unwrap_err()
            .code,
        "tool_unavailable"
    );
    update_tools_key(
        &gateway,
        &key_id,
        json!({"mcp_access_enabled":true,"is_enabled":false}),
    )
    .await;
    assert!(!tool.available(&context).await.unwrap());
    assert!(tool.call(json!({"value":7}), &context).await.is_err());
    update_tools_key(
        &gateway,
        &key_id,
        json!({"is_enabled":true,"expires_at":"2000-01-01T00:00:00Z"}),
    )
    .await;
    assert!(!tool.available(&context).await.unwrap());
    assert!(tool.call(json!({"value":7}), &context).await.is_err());
    assert!(
        !tool
            .available(&crate::mcp::McpContext::new("missing-key".into()))
            .await
            .unwrap()
    );

    let unavailable: crate::storage::DynStorage = Arc::new(crate::storage::MemoryStorage::new(
        Vec::new(),
        Vec::new(),
        Vec::new(),
    ));
    let tool = mcp_tools(&gateway.platform_plugins.snapshot(), &unavailable).remove(0);
    assert!(!tool.available(&context).await.unwrap());
    assert!(tool.call(json!({"value":7}), &context).await.is_err());
}

#[tokio::test]
async fn native_agent_plugin_tools_are_independent_of_mcp_access() {
    let directory = tempfile::tempdir().unwrap();
    let gateway = tools_gateway(directory.path()).await;
    enable_fixture(&gateway.platform_plugins).await;
    let key_id = tools_key(&gateway, false).await;
    let snapshot = gateway.platform_plugins.snapshot();
    let tool = snapshot_tools(&snapshot).remove(0);
    let id = stravia_runtime_contract::agent::VersionedToolId {
        id: tool.id().as_str().to_owned(),
        version: 1,
    };
    let native = crate::agent::PlatformToolAgentAdapter::with_id(tool, id);
    let context = || crate::agent::AgentToolContext {
        principal: Principal::new(key_id.clone()),
        turn_id: stravia_runtime_contract::agent::AgentTurnId::agent(),
        cancellation: Default::default(),
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(10),
    };
    let result = crate::agent::AgentTool::execute(&native, context(), json!({"value":7}))
        .await
        .unwrap();
    assert_eq!(result.content, json!({"value":7}));

    let mcp =
        crate::agent::McpToolAgentAdapter::new(mcp_tools(&snapshot, &gateway.storage).remove(0), 1);
    let error = crate::agent::AgentTool::execute(&mcp, context(), json!({"value":7}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "mcp_tool_unavailable");
}
