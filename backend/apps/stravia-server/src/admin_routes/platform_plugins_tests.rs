use super::tests::{contents_fixture, memory_gateway};
use super::*;
use anyhow::Context as _;
use axum::body::{Body, to_bytes};
use axum::http::Request;
use tower::ServiceExt;

#[tokio::test]
async fn platform_plugins_all_management_routes_require_admin() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let (_gateway, app, _token) = contents_fixture(directory.path()).await?;
    for (method, path, content_type, body) in [
        ("GET", "/api/v1/platform-plugins", "application/json", ""),
        (
            "POST",
            "/api/v1/platform-plugins",
            "application/wasm",
            "invalid",
        ),
        (
            "PUT",
            "/api/v1/platform-plugins/example/settings",
            "application/json",
            "{\"enabled\":true}",
        ),
        (
            "POST",
            "/api/v1/platform-plugins/example",
            "application/wasm",
            "invalid",
        ),
        (
            "DELETE",
            "/api/v1/platform-plugins/example",
            "application/json",
            "",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", content_type)
                    .body(Body::from(body))?,
            )
            .await?;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn platform_plugins_reject_invalid_uploads_without_changing_inventory() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let gateway = memory_gateway(directory.path()).await?;
    let app = create_unprotected_router(gateway);
    for (content_type, body, expected) in [
        ("application/json", "{}", StatusCode::UNSUPPORTED_MEDIA_TYPE),
        ("application/wasm", "", StatusCode::BAD_REQUEST),
        ("application/wasm", "invalid", StatusCode::BAD_REQUEST),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/v1/platform-plugins")
                    .header("content-type", content_type)
                    .body(Body::from(body))?,
            )
            .await?;
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    let response = app
        .oneshot(Request::get("/api/v1/platform-plugins").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await?)?;
    assert_eq!(body["data"], serde_json::json!([]));
    Ok(())
}

#[tokio::test]
async fn platform_plugins_http_install_update_disable_and_uninstall() -> anyhow::Result<()> {
    let fixture = std::env::var_os("STRAVIA_PLATFORM_PLUGIN_FIXTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../target/wasm32-wasip2/release/stravia_platform_plugin_example.wasm")
        });
    let component = std::fs::read(&fixture).with_context(|| {
        format!(
            "read platform plugin fixture {}; run `task build:platform-plugin-example`",
            fixture.display()
        )
    })?;
    let directory = tempfile::tempdir()?;
    let (_gateway, app, token) = contents_fixture(directory.path()).await?;
    let auth = format!("Bearer {token}");
    let upload = || {
        Request::post("/api/v1/platform-plugins")
            .header("authorization", &auth)
            .header("content-type", "application/wasm")
            .body(Body::from(component.clone()))
    };

    let response = app.clone().oneshot(upload()?).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let installed: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(installed["data"]["manifest"]["id"], "example.policy-tools");
    assert_eq!(installed["data"]["enabled"], false);
    assert_eq!(installed["data"]["sha256"].as_str().map(str::len), Some(64));
    let settings_path = "/api/v1/platform-plugins/example.policy-tools/settings";
    let response = app
        .clone()
        .oneshot(
            Request::put(settings_path)
                .header("authorization", &auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"enabled":true,"config":{"system":"Use concise answers."}}"#,
                ))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let configured: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(configured["data"]["enabled"], true);
    assert_eq!(
        configured["data"]["config"]["system"],
        "Use concise answers."
    );

    let response = app
        .clone()
        .oneshot(
            Request::put(settings_path)
                .header("authorization", &auth)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"config":null}"#))?,
        )
        .await?;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "explicit null is not an absent configuration update"
    );

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/platform-plugins/different-plugin")
                .header("authorization", &auth)
                .header("content-type", "application/wasm")
                .body(Body::from(component.clone()))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/v1/platform-plugins")
                .header("authorization", &auth)
                .body(Body::empty())?,
        )
        .await?;
    let unchanged: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(
        unchanged["data"],
        serde_json::json!([configured["data"].clone()])
    );

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/platform-plugins/example.policy-tools")
                .header("authorization", &auth)
                .header("content-type", "application/wasm")
                .body(Body::from(component.clone()))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let updated: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(
        updated["data"]["enabled"], false,
        "updates require a fresh capability review"
    );
    assert_eq!(updated["data"]["config"], configured["data"]["config"]);

    let response = app
        .clone()
        .oneshot(
            Request::delete("/api/v1/platform-plugins/example.policy-tools")
                .header("authorization", &auth)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .oneshot(
            Request::get("/api/v1/platform-plugins")
                .header("authorization", &auth)
                .body(Body::empty())?,
        )
        .await?;
    let inventory: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await?)?;
    assert_eq!(inventory["data"], serde_json::json!([]));
    Ok(())
}
