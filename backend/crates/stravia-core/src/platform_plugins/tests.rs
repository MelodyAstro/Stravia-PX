use std::path::PathBuf;
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
        .begin(context(), &old_request, ContextCompleteness::Full)
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
        .begin(context(), &new_request, ContextCompleteness::Full)
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
        .begin(context(), &disabled_request, ContextCompleteness::Full)
        .unwrap();
    manager.uninstall(id).await.unwrap();

    assert!(matches!(
        old_run.on_request(&mut old_request).await.unwrap(),
        HookControl::Continue
    ));
    assert_eq!(old_request.instructions.as_deref(), Some("old policy"));
    assert!(!old_request.tools.as_ref().unwrap().is_empty());
    new_run.on_request(&mut new_request).await.unwrap();
    assert_eq!(new_request.instructions.as_deref(), Some("new policy"));
    disabled_run
        .on_request(&mut disabled_request)
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
        .begin(context(), &request, ContextCompleteness::Full)
        .unwrap();
    assert!(run.requires_terminal_buffering());
    run.on_request(&mut request).await.unwrap();
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
    let manager = PlatformPlugins::open(directory.path()).await.unwrap();
    let registry =
        crate::mcp::McpToolRegistry::default().with_platform_plugins(Arc::clone(&manager));
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
        .call(
            json!({"value":7}),
            &crate::mcp::McpContext::new("test-key".into()),
        )
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
