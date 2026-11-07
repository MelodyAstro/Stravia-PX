use super::*;

pub(super) async fn get_request_models(
    State(gw): State<Gateway>,
    Path(id): Path<String>,
) -> axum::response::Response {
    match gw.admin().request_models(&id).await {
        Ok(view) => Json(serde_json::json!({"data":view})).into_response(),
        Err(error) => err(error),
    }
}

pub(super) async fn update_request_models(
    State(gw): State<Gateway>,
    Path(id): Path<String>,
    Json(config): Json<stravia_core::request_models::RequestModelConfig>,
) -> axum::response::Response {
    match gw.admin().update_request_models(&id, config).await {
        Ok(view) => Json(serde_json::json!({"data":view})).into_response(),
        Err(error) => err(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RequestModelCaptureInput {
    enabled: bool,
}

pub(super) async fn capture_request_models(
    State(gw): State<Gateway>,
    Path(id): Path<String>,
    Json(input): Json<RequestModelCaptureInput>,
) -> axum::response::Response {
    match gw.admin().capture_request_models(&id, input.enabled).await {
        Ok(view) => Json(serde_json::json!({"data":view})).into_response(),
        Err(error) => err(error),
    }
}

pub(super) async fn clear_request_model_capture(
    State(gw): State<Gateway>,
    Path(id): Path<String>,
) -> axum::response::Response {
    match gw.admin().clear_request_model_capture(&id).await {
        Ok(view) => Json(serde_json::json!({"data":view})).into_response(),
        Err(error) => err(error),
    }
}

// ── API Keys ──

pub(super) async fn list_api_keys_handler(State(gw): State<Gateway>) -> impl IntoResponse {
    match gw.admin().list_api_keys().await {
        Ok(v) => {
            let admin = gw.admin();
            let custom = match admin.custom_currency().await {
                Ok(custom) => custom,
                Err(e) => return err(e),
            };
            let mut items = Vec::with_capacity(v.len());
            for key in &v {
                match admin.api_key_json_with_currency(key, custom.as_ref()) {
                    Ok(value) => items.push(value),
                    Err(e) => return err(e),
                }
            }
            Json(serde_json::json!({ "data": items })).into_response()
        }
        Err(e) => err(e),
    }
}

pub(super) async fn get_api_key_handler(
    State(gw): State<Gateway>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match gw.admin().get_api_key(&id).await {
        Ok(v) => match gw.admin().api_key_json(&v).await {
            Ok(value) => Json(serde_json::json!({ "data": value })).into_response(),
            Err(e) => err(e),
        },
        Err(e) => err(e),
    }
}

pub(super) async fn create_api_key_handler(
    State(gw): State<Gateway>,
    Json(input): Json<CreateApiKey>,
) -> impl IntoResponse {
    match gw.admin().create_api_key(input).await {
        Ok(v) => match gw.admin().api_key_json(&v).await {
            Ok(value) => Json(serde_json::json!({ "data": value })).into_response(),
            Err(e) => err(e),
        },
        Err(e) => err(e),
    }
}

pub(super) async fn update_api_key_handler(
    State(gw): State<Gateway>,
    Path(id): Path<String>,
    Json(input): Json<UpdateApiKey>,
) -> impl IntoResponse {
    match gw.admin().update_api_key(&id, input).await {
        Ok(v) => match gw.admin().api_key_json(&v).await {
            Ok(value) => Json(serde_json::json!({ "data": value })).into_response(),
            Err(e) => err(e),
        },
        Err(e) => err(e),
    }
}

pub(super) async fn reset_api_key_spend_handler(
    State(gw): State<Gateway>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match gw.admin().reset_api_key_spend(&id).await {
        Ok(v) => match gw.admin().api_key_spend_json(&v).await {
            Ok(value) => Json(serde_json::json!({ "data": value })).into_response(),
            Err(e) => err(e),
        },
        Err(e) => err(e),
    }
}

pub(super) async fn delete_api_key_handler(
    State(gw): State<Gateway>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match gw.admin().delete_api_key(&id).await {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => err(e),
    }
}
