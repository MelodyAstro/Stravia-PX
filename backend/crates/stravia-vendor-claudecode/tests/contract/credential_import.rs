use super::*;

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires freshly built managed components"]
async fn credential_import_refreshes_codex_and_google_and_preserves_client_metadata() {
    for (artifact, vendor, channel, base, token_url, client_id, extra) in [
        (
            "STRAVIA_CODEX_WASM",
            "openai-codex",
            "codex",
            "https://chatgpt.com/backend-api/codex",
            "https://auth.openai.com/oauth/token",
            "app_fixtureClient",
            json!({"account_id":"selected-team", "id_token":"fixture.id.signature"}),
        ),
        (
            "STRAVIA_ANTIGRAVITY_WASM",
            "antigravity",
            "oauth",
            "https://daily-cloudcode-pa.googleapis.com",
            "https://oauth2.googleapis.com/token",
            "fixture.apps.googleusercontent.com",
            json!({"project_id":"selected-project", "client_secret":"fixture-public-client-secret"}),
        ),
    ] {
        let runtime = VendorRuntime::new().unwrap();
        let path = std::env::var_os(artifact).expect("managed component path");
        let plugin = runtime.load(&std::fs::read(path).unwrap()).await.unwrap();
        let services = Arc::new(MockServices { script: Mutex::new(VecDeque::from([
            (200, "application/json", json!({"access_token":"fresh-access", "refresh_token":"rotated-refresh", "expires_in":3600}).to_string(), None),
            (200, "application/json", json!({"access_token":"next-access", "expires_in":3600}).to_string(), None),
        ])), ..Default::default() });
        let mut account = provider(BTreeMap::new());
        account.provider_id = vendor.into();
        account.channel = channel.into();
        account.base_url = base.into();
        let mut payload = extra;
        payload["refresh_token"] = json!("original-refresh");
        payload["client_id"] = json!(client_id);
        let output = run(
            &runtime,
            &plugin,
            &services,
            OperationInput::Auth {
                provider: account.clone(),
                request: AuthRequest {
                    step: AuthStep::ManualInput {
                        value: payload.to_string(),
                    },
                },
            },
        )
        .await
        .unwrap();
        let OperationOutput::Auth(AuthResponse::Credentials {
            values,
            expires_at_unix_ms,
        }) = output
        else {
            panic!("expected imported credentials");
        };
        assert!(expires_at_unix_ms.is_some());
        assert_eq!(values["refresh_token"], "rotated-refresh");
        assert_eq!(values["client_id"], client_id);
        if vendor == "openai-codex" {
            assert_eq!(values["account_id"], "selected-team");
            assert_eq!(values["raw"]["id_token"], "fixture.id.signature");
        } else {
            assert_eq!(values["project_id"], "selected-project");
        }
        account.credentials = values;
        let output = run(
            &runtime,
            &plugin,
            &services,
            OperationInput::Auth {
                provider: account,
                request: AuthRequest {
                    step: AuthStep::Refresh,
                },
            },
        )
        .await
        .unwrap();
        let OperationOutput::Auth(AuthResponse::Credentials { values, .. }) = output else {
            panic!("expected refreshed credentials");
        };
        assert_eq!(values["refresh_token"], "rotated-refresh");
        let requests = services.requests.lock();
        assert_eq!(requests.len(), 2);
        for request in requests.iter() {
            assert_eq!(request.url, token_url);
            let form: BTreeMap<_, _> = url::form_urlencoded::parse(&request.body)
                .into_owned()
                .collect();
            assert_eq!(form["client_id"], client_id);
        }
        let second: BTreeMap<_, _> = url::form_urlencoded::parse(&requests[1].body)
            .into_owned()
            .collect();
        assert_eq!(second["refresh_token"], "rotated-refresh");
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires freshly built managed components"]
async fn credential_import_access_only_codex_verifies_credentials_without_inference() {
    let runtime = VendorRuntime::new().unwrap();
    let plugin = runtime
        .load(&std::fs::read(std::env::var_os("STRAVIA_CODEX_WASM").unwrap()).unwrap())
        .await
        .unwrap();
    let services = Arc::new(MockServices {
        script: Mutex::new(VecDeque::from([(
            200,
            "application/json",
            json!({"rate_limit":{"primary_window":null},"plan_type":"team"}).to_string(),
            None,
        )])),
        ..Default::default()
    });
    let mut account = provider(BTreeMap::new());
    account.provider_id = "openai-codex".into();
    account.channel = "codex".into();
    account.base_url = "https://chatgpt.com/backend-api/codex".into();
    let output = run(
        &runtime,
        &plugin,
        &services,
        OperationInput::Auth {
            provider: account,
            request: AuthRequest {
                step: AuthStep::ManualInput {
                    value: json!({"access_token":"fixture-access", "account_id":"selected-team"})
                        .to_string(),
                },
            },
        },
    )
    .await
    .unwrap();
    let OperationOutput::Auth(AuthResponse::Credentials { values, .. }) = output else {
        panic!("expected credentials");
    };
    assert_eq!(values["account_id"], "selected-team");
    assert!(!values.contains_key("refresh_token"));
    let requests = services.requests.lock();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].url,
        "https://chatgpt.com/backend-api/wham/usage"
    );
    assert_eq!(requests[0].method, "GET");
}
