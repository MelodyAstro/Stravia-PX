//! Explicit manual reset redemption using the official backend-client contract.
use serde_json::{Value, json};
use stravia_vendor_common::common::{plugin_error, upstream_error};
use stravia_vendor_sdk::{
    AllowanceAction, AllowanceActionResult, AllowanceResponse, GuestHost, PluginError,
    ProviderSnapshot, read_http_body,
};

pub(crate) fn execute(
    host: &GuestHost,
    provider: &ProviderSnapshot,
    action: AllowanceAction,
) -> Result<AllowanceResponse, PluginError> {
    let AllowanceAction::RedeemReset { request_id } = action;
    uuid::Uuid::parse_str(&request_id).map_err(|_| {
        plugin_error(
            stravia_vendor_sdk::ErrorKind::Invalid,
            "A stable reset request UUID is required",
        )
    })?;
    let mut receipt = stravia_vendor_common::account_action::Receipt::open(&request_id)?;
    if let Some(code) = receipt.result.clone() {
        return Ok(result(code, request_id));
    }
    receipt.record_target(None)?;
    let mut request =
        super::allowance::usage_request(provider, super::allowance::access_token(provider)?);
    request.method = "POST".into();
    request.url = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume".into();
    request
        .headers
        .set("content-type", b"application/json".to_vec());
    request.body = serde_json::to_vec(&json!({"redeem_request_id":request_id})).map_err(|_| {
        plugin_error(
            stravia_vendor_sdk::ErrorKind::Invalid,
            "Reset request encoding failed",
        )
    })?;
    let response = host.http_start(request, &super::codex::http_options()?)?;
    let status = response.status()?;
    let headers = response.headers()?;
    let body = read_http_body(&response, 64 * 1024)?;
    if !(200..300).contains(&status) {
        return Err(upstream_error(status, &headers, &body));
    }
    let payload: Value = serde_json::from_slice(&body).map_err(|_| {
        plugin_error(
            stravia_vendor_sdk::ErrorKind::Invalid,
            "Reset result is unreadable; retain the request id",
        )
    })?;
    let code = payload["code"]
        .as_str()
        .filter(|code| {
            ["reset", "nothing_to_reset", "no_credit", "already_redeemed"].contains(code)
        })
        .ok_or_else(|| {
            plugin_error(
                stravia_vendor_sdk::ErrorKind::Invalid,
                "Unknown reset outcome; retain the request id",
            )
        })?;
    // Do not turn a refresh failure into an unknown mutation or auto-redeem again.
    receipt.finish(code)?;
    Ok(result(code.into(), request_id))
}

fn result(code: String, request_id: String) -> AllowanceResponse {
    AllowanceResponse {
        action_result: Some(AllowanceActionResult { code, request_id }),
        allowances: Vec::new(),
        models: Vec::new(),
        plan_label: None,
        account_label: None,
        billing: None,
    }
}
