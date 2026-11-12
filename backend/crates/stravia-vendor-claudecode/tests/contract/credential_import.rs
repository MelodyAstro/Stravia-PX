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
        let services = Arc::new(MockServices { reject_provider_cookies: true, script: Mutex::new(VecDeque::from([
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
            // Fixture client IDs contain only unreserved form characters.
            let form = std::str::from_utf8(&request.body).unwrap();
            assert_eq!(
                form.split('&')
                    .find_map(|field| field.strip_prefix("client_id=")),
                Some(client_id)
            );
        }
        let second = std::str::from_utf8(&requests[1].body).unwrap();
        assert_eq!(
            second
                .split('&')
                .find_map(|field| field.strip_prefix("refresh_token=")),
            Some("rotated-refresh")
        );
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
        reject_provider_cookies: true,
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

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires freshly built managed components"]
async fn credential_import_codex_callback_preserves_pkce_and_upstream_rejections() {
    let runtime = VendorRuntime::new().unwrap();
    let plugin = runtime
        .load(&std::fs::read(std::env::var_os("STRAVIA_CODEX_WASM").unwrap()).unwrap())
        .await
        .unwrap();
    for status in [200, 400] {
        let payload = if status == 200 {
            json!({"access_token":"fixture-access","refresh_token":"fixture-refresh","expires_in":3600})
        } else {
            json!({"error":"invalid_grant","error_description":"invalid authorization code"})
        };
        let services = Arc::new(MockServices {
            reject_provider_cookies: true,
            script: Mutex::new(VecDeque::from([(
                status,
                "application/json",
                payload.to_string(),
                None,
            )])),
            ..Default::default()
        });
        let mut account = provider(BTreeMap::new());
        account.provider_id = "openai-codex".into();
        account.channel = "codex".into();
        account.base_url = "https://chatgpt.com/backend-api/codex".into();
        let redirect = "http://localhost:1457/auth/callback";
        let start = run(
            &runtime,
            &plugin,
            &services,
            OperationInput::Auth {
                provider: account.clone(),
                request: AuthRequest {
                    step: AuthStep::Start {
                        redirect_uri: redirect.into(),
                        state: "fixture-state".into(),
                    },
                },
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            start,
            OperationOutput::Auth(AuthResponse::Authorization { .. })
        ));
        assert!(services.requests.lock().is_empty());
        let saved: Value =
            serde_json::from_slice(services.private_state.lock().as_ref().unwrap()).unwrap();
        let verifier = saved["code_verifier"].as_str().unwrap();
        assert_eq!(verifier.len(), 96);
        let output = run(
            &runtime,
            &plugin,
            &services,
            OperationInput::Auth {
                provider: account,
                request: AuthRequest {
                    step: AuthStep::Exchange {
                        callback_url: format!("{redirect}?state=fixture-state&code=fixture-code"),
                    },
                },
            },
        )
        .await;
        if status == 200 {
            let OperationOutput::Auth(AuthResponse::Credentials { values, .. }) = output.unwrap() else {
                panic!("expected exchanged credentials");
            };
            assert_eq!(values["access_token"], "fixture-access");
            assert_eq!(values["refresh_token"], "fixture-refresh");
        } else {
            assert!(
                matches!(
                    &output,
                    Err(RuntimeError::Plugin {
                        kind: ErrorKind::Auth,
                        upstream_status: Some(400),
                        ..
                    })
                ),
                "{output:?}"
            );
        }
        let requests = services.requests.lock();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, "https://auth.openai.com/oauth/token");
        let form = std::str::from_utf8(&requests[0].body).unwrap();
        assert!(form.contains("grant_type=authorization_code"));
        assert!(form.contains("code=fixture-code"));
        assert!(form.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1457%2Fauth%2Fcallback"));
        assert!(form.contains(&format!("code_verifier={verifier}")));
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires freshly built managed components"]
async fn credential_import_codex_draft_connection_check_keeps_session_state() {
    let runtime = VendorRuntime::new().unwrap();
    let plugin = runtime
        .load(&std::fs::read(std::env::var_os("STRAVIA_CODEX_WASM").unwrap()).unwrap())
        .await
        .unwrap();
    let before = b"fixture-session-state".to_vec();
    let services = Arc::new(MockServices {
        reject_provider_cookies: true,
        private_state: Mutex::new(Some(before.clone())),
        script: Mutex::new(VecDeque::from([(
            200,
            "application/json",
            json!({"plan_type":"team"}).to_string(),
            None,
        )])),
        ..Default::default()
    });
    let mut account = provider(BTreeMap::from([(
        "access_token".into(),
        json!("fixture-access"),
    )]));
    account.provider_id = "openai-codex".into();
    account.channel = "codex".into();
    account.base_url = "https://chatgpt.com/backend-api/codex".into();
    let output = run(
        &runtime,
        &plugin,
        &services,
        OperationInput::ConnectionCheck {
            provider: account,
            request: stravia_vendor_sdk::ConnectionCheckRequest {},
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        output,
        OperationOutput::ConnectionCheck(stravia_vendor_sdk::ConnectionCheckResponse {
            verified: stravia_vendor_sdk::ConnectionCheckVerification::Credentials,
        })
    ));
    assert_eq!(*services.private_state.lock(), Some(before));
    assert!(services.deltas.lock().is_empty());
    assert_eq!(services.requests.lock().len(), 1);
    assert_eq!(services.wire_log.lock().as_slice(), ["request"]);
}
