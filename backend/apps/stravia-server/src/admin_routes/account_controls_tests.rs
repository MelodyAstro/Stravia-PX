use super::tests::contents_fixture;
use super::*;
use axum::{body::Body, http::Request};
use tower::ServiceExt;

#[tokio::test]
async fn account_controls_require_admin_and_mutations_require_explicit_confirmation()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let (_, app, token) = contents_fixture(directory.path()).await?;
    for (method, path, body) in [
        ("GET", "/api/v1/provider-allowances/example/reset", ""),
        (
            "POST",
            "/api/v1/provider-allowances/example/reset",
            r#"{"request_id":"00000000-0000-4000-8000-000000000001","confirmed":true}"#,
        ),
        ("GET", "/api/v1/provider-allowances/example/budget", ""),
        (
            "PUT",
            "/api/v1/provider-allowances/example/budget",
            r#"{"limit":"1","expected_revision":0}"#,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body))?,
            )
            .await?;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
    for body in [
        r#"{"request_id":"00000000-0000-4000-8000-000000000001","confirmed":false}"#,
        r#"{"request_id":"not-a-uuid","confirmed":true}"#,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/v1/provider-allowances/example/reset")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    Ok(())
}

#[tokio::test]
async fn account_controls_credential_import_is_admin_only_and_preview_never_echoes_tokens()
-> anyhow::Result<()> {
    use axum::body::to_bytes;
    let directory = tempfile::tempdir()?;
    let (gateway, app, token) = contents_fixture(directory.path()).await?;
    let document = serde_json::json!({"type":"codex", "refresh_token":"sensitive-import-fixture", "email":"fixture@example.invalid", "proxy":"https://ignored.invalid"}).to_string();
    let payload = serde_json::json!({"kind":"codex", "json":document}).to_string();
    for path in [
        "/api/v1/providers/credential-import/preview",
        "/api/v1/providers/credential-import",
        "/api/v1/providers/example/rt-backup",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(payload.clone()))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/providers/credential-import/preview")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(payload.clone()))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), 4096).await?;
    let body: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(body["data"]["rows"][0]["code"], "IMPORT_READY");
    assert!(!String::from_utf8_lossy(&bytes).contains("sensitive-import-fixture"));
    assert!(!String::from_utf8_lossy(&bytes).contains("ignored.invalid"));
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/providers/credential-import")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(payload))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(gateway.admin().list_providers().await?.is_empty());
    Ok(())
}
