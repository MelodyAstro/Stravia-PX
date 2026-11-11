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
