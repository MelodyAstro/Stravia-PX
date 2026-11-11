//! Manual Claude reset redemption; inventory and next grant are read live.
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
    uuid::Uuid::parse_str(&request_id)
        .map_err(|_| invalid("A stable reset request UUID is required"))?;
    let organization = provider
        .credentials
        .get("organization_uuid")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("Claude organization is unavailable"))?;
    uuid::Uuid::parse_str(organization).map_err(|_| invalid("Claude organization is invalid"))?;
    let mut receipt = stravia_vendor_common::account_action::Receipt::open(&request_id)?;
    if let Some(code) = receipt.result.clone() {
        return Ok(result(code, request_id));
    }
    let grant = if let Some(grant) = receipt.target.clone() {
        grant
    } else {
        let response = host.http_start(
            super::allowance::usage_request(provider, super::access_token(provider)?, true),
            &super::net_options(),
        )?;
        let status = response.status()?;
        let headers = response.headers()?;
        let body = read_http_body(&response, 1024 * 1024)?;
        if !(200..300).contains(&status) {
            return Err(upstream_error(status, &headers, &body));
        }
        let payload: Value =
            serde_json::from_slice(&body).map_err(|_| invalid("Reset inventory is unreadable"))?;
        let reset = &payload["cedar_ember"];
        let grant = reset["next_grant_id"]
            .as_str()
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 40
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
            .ok_or_else(|| invalid("No reset is currently redeemable"))?;
        if reset["eligible"].as_bool() != Some(true)
            || !reset["grants"].as_array().is_some_and(|grants| {
                grants.iter().any(|item| {
                    item["id"].as_str() == Some(grant)
                        && item["usable_now"].as_bool() == Some(true)
                        && item["paused"].as_bool() == Some(false)
                        && item["resets_left"].as_u64().is_some_and(|n| n > 0)
                })
            })
        {
            return Err(invalid("No reset is currently redeemable"));
        }
        let grant = grant.to_owned();
        receipt.record_target(Some(grant.clone()))?;
        grant
    };
    let mut headers = vec![
        (
            "authorization".into(),
            format!("Bearer {}", super::access_token(provider)?),
        ),
        ("anthropic-beta".into(), "oauth-2025-04-20".into()),
        ("content-type".into(), "application/json".into()),
    ];
    stravia_vendor_common::identity::apply_claude(
        provider,
        &mut headers,
        super::DEFAULT_CLIENT_VERSION,
    );
    let request = super::net_request(
        "POST",
        format!("https://api.anthropic.com/api/organizations/{organization}/reset_rate_limits"),
        headers,
        serde_json::to_vec(
            &json!({"program":"cedar_ember","grant_id":grant,"request_id":request_id}),
        )
        .map_err(|_| invalid("Reset request encoding failed"))?,
    );
    let response = host.http_start(request, &super::net_options())?;
    let status = response.status()?;
    let headers = response.headers()?;
    let body = read_http_body(&response, 64 * 1024)?;
    if !(200..300).contains(&status) {
        return Err(upstream_error(status, &headers, &body));
    }
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| invalid("Reset result unreadable; retain the request id"))?;
    let code = payload["result"]
        .as_str()
        .filter(|code| {
            [
                "reset",
                "already_used",
                "not_limited",
                "cooldown",
                "ineligible",
                "unavailable",
            ]
            .contains(code)
        })
        .ok_or_else(|| invalid("Unknown reset outcome; retain the request id"))?;
    receipt.finish(code)?;
    Ok(result(code.into(), request_id))
}
fn invalid(message: &str) -> PluginError {
    plugin_error(stravia_vendor_sdk::ErrorKind::Invalid, message)
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
