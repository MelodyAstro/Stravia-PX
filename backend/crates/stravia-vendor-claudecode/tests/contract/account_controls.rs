use super::*;
use stravia_vendor_sdk::{AllowanceAction, AllowanceRequest};

fn reset_request(provider: ProviderSnapshot, id: &str) -> OperationInput {
    OperationInput::Allowance {
        provider,
        request: AllowanceRequest {
            model: None,
            action: Some(AllowanceAction::RedeemReset {
                request_id: id.into(),
            }),
        },
    }
}
fn claude_account() -> ProviderSnapshot {
    let mut provider = signed_in();
    provider
        .credentials
        .insert("organization_uuid".into(), json!(ACCOUNT_UUID));
    provider
}
fn inventory() -> String {
    json!({"cedar_ember":{"eligible":true,"next_grant_id":"grant-1","grants":[{"id":"grant-1","resets_left":2,"resets_total":2,"paused":false,"usable_now":true,"starts_at":null,"ends_at":null}]}}).to_string()
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires freshly built managed components"]
async fn account_reset_claude_replay_does_not_consume_twice() {
    let (runtime, plugin) = load_plugin().await;
    let services = Arc::new(MockServices {
        script: Mutex::new(VecDeque::from([
            (200, "application/json", inventory(), None),
            (
                200,
                "application/json",
                json!({"result":"reset"}).to_string(),
                None,
            ),
        ])),
        ..Default::default()
    });
    let id = "00000000-0000-4000-8000-000000000011";
    for _ in 0..2 {
        let output = run(
            &runtime,
            &plugin,
            &services,
            reset_request(claude_account(), id),
        )
        .await
        .unwrap();
        let OperationOutput::Allowance(response) = output else {
            panic!("expected reset acknowledgement")
        };
        let result = response.action_result.unwrap();
        assert_eq!(result.code, "reset");
        assert_eq!(result.request_id, id);
    }
    let requests = services.requests.lock();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].method, "POST");
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[1].body).unwrap(),
        json!({"program":"cedar_ember","grant_id":"grant-1","request_id":id})
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires freshly built managed components"]
async fn account_reset_unknown_outcome_keeps_grant_and_id_and_rejects_rebinding() {
    let (runtime, plugin) = load_plugin().await;
    let services = Arc::new(MockServices {
        script: Mutex::new(VecDeque::from([
            (200, "application/json", inventory(), None),
            (200, "application/json", "{}".into(), None),
            (
                200,
                "application/json",
                json!({"result":"already_used"}).to_string(),
                None,
            ),
        ])),
        ..Default::default()
    });
    let id = "00000000-0000-4000-8000-000000000012";
    assert!(
        run(
            &runtime,
            &plugin,
            &services,
            reset_request(claude_account(), id)
        )
        .await
        .is_err()
    );
    assert!(
        run(
            &runtime,
            &plugin,
            &services,
            reset_request(claude_account(), "00000000-0000-4000-8000-000000000099")
        )
        .await
        .is_err()
    );
    let mut rebound = claude_account();
    rebound.credentials.insert(
        "organization_uuid".into(),
        json!("00000000-0000-4000-8000-0000000000bb"),
    );
    assert!(
        run(&runtime, &plugin, &services, reset_request(rebound, id))
            .await
            .is_err()
    );
    assert_eq!(services.requests.lock().len(), 2);
    let output = run(
        &runtime,
        &plugin,
        &services,
        reset_request(claude_account(), id),
    )
    .await
    .unwrap();
    let OperationOutput::Allowance(response) = output else {
        panic!("expected acknowledged result")
    };
    assert_eq!(response.action_result.unwrap().code, "already_used");
    let requests = services.requests.lock();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].body, requests[2].body);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires freshly built managed components"]
async fn account_reset_codex_only_accepts_known_result_codes_and_reuses_request_id() {
    let path = std::env::var_os("STRAVIA_CODEX_WASM").expect("built Codex component");
    let runtime = VendorRuntime::new().unwrap();
    let plugin = runtime.load(&std::fs::read(path).unwrap()).await.unwrap();
    let mut provider = claude_account();
    provider.provider_id = "openai-codex".into();
    provider.channel = "codex".into();
    provider.protocol = "openai-responses".into();
    provider.base_url = "https://chatgpt.com/backend-api/codex".into();
    provider.credentials = BTreeMap::from([
        ("access_token".into(), json!("fixture-access-token")),
        ("account_id".into(), json!("fixture-account")),
    ]);
    let services = Arc::new(MockServices {
        script: Mutex::new(VecDeque::from([
            (
                200,
                "application/json",
                json!({"code":"server_changed"}).to_string(),
                None,
            ),
            (
                200,
                "application/json",
                json!({"code":"already_redeemed"}).to_string(),
                None,
            ),
        ])),
        ..Default::default()
    });
    let id = "00000000-0000-4000-8000-000000000013";
    assert!(
        run(
            &runtime,
            &plugin,
            &services,
            reset_request(provider.clone(), id)
        )
        .await
        .is_err()
    );
    for _ in 0..2 {
        let output = run(
            &runtime,
            &plugin,
            &services,
            reset_request(provider.clone(), id),
        )
        .await
        .unwrap();
        let OperationOutput::Allowance(response) = output else {
            panic!("expected reset acknowledgement")
        };
        assert_eq!(response.action_result.unwrap().code, "already_redeemed");
    }
    let requests = services.requests.lock();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.url
        == "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume"));
    assert_eq!(requests[0].body, requests[1].body);
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[0].body).unwrap(),
        json!({"redeem_request_id":id})
    );
}
