use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use stravia_core::Gateway;
use stravia_core::admin::provider_allowance::ProviderAllowanceGuardError;

#[derive(Deserialize)]
pub(super) struct ReplaceProviderAllowanceGuards {
    keys: Vec<String>,
}

pub(super) async fn replace_provider_allowance_guards(
    State(gateway): State<Gateway>,
    Path(provider_id): Path<String>,
    Json(input): Json<ReplaceProviderAllowanceGuards>,
) -> impl IntoResponse {
    match gateway
        .admin()
        .replace_provider_allowance_guards(&provider_id, input.keys)
        .await
    {
        Ok(Some(update)) => Json(serde_json::json!({ "data": update })).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "provider not found",
                "code": "PROVIDER_NOT_FOUND",
            })),
        )
            .into_response(),
        Err(error)
            if error
                .downcast_ref::<ProviderAllowanceGuardError>()
                .is_some() =>
        {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": error.to_string(),
                    "code": "PROVIDER_ALLOWANCE_GUARDS_INVALID",
                })),
            )
                .into_response()
        }
        Err(error) => {
            tracing::error!(provider_id, %error, "Failed to replace provider allowance guards");
            internal_error(
                "PROVIDER_ALLOWANCE_GUARDS_SAVE_FAILED",
                "failed to save provider allowance guards",
            )
        }
    }
}

pub(super) async fn list_provider_allowances(State(gateway): State<Gateway>) -> impl IntoResponse {
    match gateway.admin().list_provider_allowance_targets().await {
        Ok(targets) => Json(serde_json::json!({ "data": targets })).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "Failed to list provider allowances");
            internal_error(
                "PROVIDER_ALLOWANCE_LOAD_FAILED",
                "failed to load provider allowances",
            )
        }
    }
}

pub(super) async fn get_provider_allowance(
    State(gateway): State<Gateway>,
    Path(provider_id): Path<String>,
) -> impl IntoResponse {
    match gateway.admin().get_provider_allowance(&provider_id).await {
        Ok(Some(snapshot)) => Json(serde_json::json!({ "data": snapshot })).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "provider allowance is unavailable",
                "code": "PROVIDER_ALLOWANCE_UNAVAILABLE",
            })),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(
                provider_id,
                error = %error,
                "Failed to load provider allowance"
            );
            internal_error(
                "PROVIDER_ALLOWANCE_LOAD_FAILED",
                "failed to load provider allowance",
            )
        }
    }
}

pub(super) async fn refresh_provider_allowance(
    State(gateway): State<Gateway>,
    Path(provider_id): Path<String>,
) -> impl IntoResponse {
    match gateway
        .admin()
        .refresh_provider_allowance(&provider_id)
        .await
    {
        Ok(Some(snapshot)) => Json(serde_json::json!({ "data": snapshot })).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "provider allowance is unavailable",
                "code": "PROVIDER_ALLOWANCE_UNAVAILABLE",
            })),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(
                provider_id,
                error = %error,
                "Failed to refresh provider allowance"
            );
            internal_error(
                "PROVIDER_ALLOWANCE_REFRESH_FAILED",
                "failed to refresh provider allowance",
            )
        }
    }
}

fn internal_error(code: &'static str, message: &'static str) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "error": message,
            "code": code,
        })),
    )
        .into_response()
}

pub(super) async fn provider_reset_state(
    State(gateway): State<Gateway>,
    Path(provider_id): Path<String>,
) -> impl IntoResponse {
    match gateway.admin().provider_reset_state(&provider_id).await {
        Ok(state) => Json(serde_json::json!({"data":state})).into_response(),
        Err(_) => internal_error("ACCOUNT_RESET_STATE_FAILED", "Could not read reset state"),
    }
}

pub(super) async fn get_account_budget(
    State(gateway): State<Gateway>,
    Path(provider_id): Path<String>,
) -> impl IntoResponse {
    match gateway.admin().account_budget(&provider_id).await {
        Ok(view) => Json(serde_json::json!({"data":view})).into_response(),
        Err(error) => (
            StatusCode::CONFLICT,
            Json(
                serde_json::json!({"code":"ACCOUNT_BUDGET_UNAVAILABLE","error":error.to_string()}),
            ),
        )
            .into_response(),
    }
}

pub(super) async fn set_account_budget(
    State(gateway): State<Gateway>,
    Path(provider_id): Path<String>,
    Json(input): Json<stravia_core::account_usage::AccountBudgetInput>,
) -> impl IntoResponse {
    match gateway
        .admin()
        .set_account_budget(&provider_id, input)
        .await
    {
        Ok(view) => Json(serde_json::json!({"data":view})).into_response(),
        Err(error) => (
            StatusCode::CONFLICT,
            Json(
                serde_json::json!({"code":"ACCOUNT_BUDGET_SAVE_FAILED","error":error.to_string()}),
            ),
        )
            .into_response(),
    }
}

pub(super) async fn redeem_provider_reset(
    State(gateway): State<Gateway>,
    Path(provider_id): Path<String>,
    Json(input): Json<stravia_core::admin::provider_allowance::ManualResetInput>,
) -> impl IntoResponse {
    match gateway
        .admin()
        .redeem_provider_reset(&provider_id, input)
        .await
    {
        Ok(result) => Json(serde_json::json!({"data":result})).into_response(),
        Err(error) => {
            tracing::warn!(provider_id, %error,"Account reset did not produce a confirmed result");
            (StatusCode::CONFLICT,Json(serde_json::json!({"code":"ACCOUNT_RESET_UNCONFIRMED","error":"Reset could not be confirmed. Refresh its state and reuse the same request id."}))).into_response()
        }
    }
}
