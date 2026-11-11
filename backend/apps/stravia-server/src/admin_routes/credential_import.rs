use super::*;
use stravia_core::admin::CredentialImportInput;

pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route("/providers/credential-import/preview", post(preview))
        .route("/providers/credential-import", post(import))
        .route("/providers/{id}/rt-backup", post(backup))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(middleware::from_fn(super::no_store))
}

fn rejected() -> axum::response::Response {
    (StatusCode::CONFLICT, Json(serde_json::json!({"error":{"code":"CREDENTIAL_IMPORT_FAILED", "message":"Credential import failed. Check the format, account duplication, and upstream authorization."}}))).into_response()
}

async fn preview(
    State(gw): State<Gateway>,
    Json(input): Json<CredentialImportInput>,
) -> axum::response::Response {
    match gw.admin().preview_credential_import(input).await {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(_) => rejected(),
    }
}

async fn import(
    State(gw): State<Gateway>,
    Json(input): Json<CredentialImportInput>,
) -> axum::response::Response {
    match gw.admin().import_credential(input).await {
        Ok(provider) => match super::providers::provider_value(&gw, provider).await {
            Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
            Err(_) => rejected(),
        },
        Err(_) => rejected(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupInput {
    confirmed: bool,
}

async fn backup(
    State(gw): State<Gateway>,
    Path(id): Path<String>,
    Json(input): Json<BackupInput>,
) -> axum::response::Response {
    match gw.admin().export_antigravity_rt(&id, input.confirmed).await {
        Ok(data) => {
            let mut response = Json(serde_json::json!({"data":data})).into_response();
            response.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"antigravity-rt.json\""),
            );
            response
        }
        Err(_) => rejected(),
    }
}
