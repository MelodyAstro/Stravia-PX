use super::*;

#[tokio::test]
async fn plugin_tools_require_mcp_access_for_listing_and_calls() {
    let app = test_app().await;
    let path = std::env::var_os("STRAVIA_PLATFORM_PLUGIN_FIXTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../target/wasm32-wasip2/release/stravia_platform_plugin_example.wasm")
        });
    let bytes = std::fs::read(path).expect("build task build:platform-plugin-example");
    let installed = app
        .gateway
        .admin()
        .install_platform_plugin(bytes)
        .await
        .expect("install plugin");
    app.gateway
        .admin()
        .update_platform_plugin(
            &installed.manifest.id,
            crate::platform_plugins::PlatformPluginUpdate {
                enabled: Some(true),
                config: None,
            },
        )
        .await
        .expect("enable plugin");
    let name = app
        .gateway
        .mcp_registry
        .current_tools()
        .keys()
        .find(|name| name.starts_with("plugin_echo_"))
        .cloned()
        .expect("plugin tool identity");
    let client = connect(&app).await;
    assert!(
        !client
            .list_tools(None)
            .await
            .expect("tools/list")
            .tools
            .iter()
            .any(|tool| tool.name.as_ref() == name.as_str())
    );
    let denied = client
        .call_tool(CallToolRequestParams::new(name.clone()).with_arguments(arguments(7)))
        .await
        .expect("denied tools/call result");
    assert_eq!(denied.is_error, Some(true));
    assert_eq!(
        denied
            .meta
            .as_ref()
            .and_then(|meta| meta.get("stravia/errorCode"))
            .and_then(Value::as_str),
        Some("tool_unavailable")
    );

    app.gateway
        .admin()
        .update_api_key(
            &app.key_id,
            serde_json::from_value(
                json!({"mcp_access_enabled":true,"transparent_injection_enabled":false}),
            )
            .unwrap(),
        )
        .await
        .expect("allow MCP independently of transparent injection");
    assert!(
        client
            .list_tools(None)
            .await
            .expect("allowed tools/list")
            .tools
            .iter()
            .any(|tool| tool.name.as_ref() == name.as_str())
    );
    let allowed = client
        .call_tool(CallToolRequestParams::new(name.clone()).with_arguments(arguments(7)))
        .await
        .expect("allowed tools/call");
    assert_ne!(allowed.is_error, Some(true));
    assert_eq!(
        allowed.structured_content,
        Some(json!({"result":{"value":7}}))
    );

    app.gateway
        .admin()
        .update_api_key(
            &app.key_id,
            serde_json::from_value(json!({"mcp_access_enabled":false})).unwrap(),
        )
        .await
        .expect("revoke MCP without reconnecting");
    assert!(
        !client
            .list_tools(None)
            .await
            .expect("revoked tools/list")
            .tools
            .iter()
            .any(|tool| tool.name.as_ref() == name.as_str())
    );
    let denied = client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(7)))
        .await
        .expect("revoked tools/call result");
    assert_eq!(denied.is_error, Some(true));
}
