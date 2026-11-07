use super::*;
use crate::request_models::{RequestModelConfig, RequestModelMapping};

async fn bound_key(
    gateway: &Gateway,
    name: &str,
    route_id: &str,
) -> crate::db::models::ApiKeyWithBindings {
    gateway
        .admin()
        .create_api_key(
            serde_json::from_value(serde_json::json!({"name":name,"model_ids":[route_id]}))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn headers(key: &crate::db::models::ApiKeyWithBindings) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", key.token)).unwrap(),
    );
    headers
}

fn config(target: &str) -> RequestModelConfig {
    RequestModelConfig {
        enabled: true,
        mappings: vec![RequestModelMapping {
            request_model: "gpt-6-luna".into(),
            target_model_id: target.into(),
        }],
    }
}

#[tokio::test]
async fn request_models_route_same_bare_name_by_key_and_preserve_response_name() {
    let data_dir = tempfile::tempdir().unwrap();
    let gateway = Gateway::new(crate::config::GatewayConfig {
        data_dir: data_dir.path().into(),
        ..Default::default()
    })
    .await
    .unwrap();
    let (private_url, private_calls) = serve_sse_sequence(vec![
        openai_sse("private reply"),
        openai_sse("private stream"),
        openai_sse("custom reply"),
    ])
    .await;
    let (team_url, team_calls) = serve_sse_sequence(vec![openai_sse("team reply")]).await;
    let private = configure_route_with_id(&gateway, "private/gpt-6-luna", &[private_url]).await;
    let team = configure_route_with_id(&gateway, "team/gpt-6-luna", &[team_url]).await;
    let private_key = bound_key(&gateway, "Private", &private).await;
    let team_key = bound_key(&gateway, "Team", &team).await;
    gateway
        .admin()
        .update_request_models(&private_key.id, config(&private))
        .await
        .unwrap();
    gateway
        .admin()
        .update_request_models(&team_key.id, config(&team))
        .await
        .unwrap();
    for (key, expected) in [(&private_key, "private reply"), (&team_key, "team reply")] {
        let response = execute_non_stream_request_with_headers(
            gateway.clone(),
            headers(key),
            AiRequest::new("gpt-6-luna", Vec::new()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(body["model"], "gpt-6-luna");
        assert!(body.to_string().contains(expected), "{body}");
        let listed = crate::proxy::handler::models_list(State(gateway.clone()), headers(key)).await;
        let listed: serde_json::Value =
            serde_json::from_slice(&to_bytes(listed.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(listed["data"].as_array().unwrap().len(), 1);
        assert_eq!(listed["data"][0]["id"], "gpt-6-luna");
    }
    let mut request = AiRequest::new("gpt-6-luna", Vec::new());
    request.stream.enabled = true;
    let stream = execute_request_with_headers(
        gateway.clone(),
        headers(&private_key),
        request,
        OPEN_RESPONSES_2026_04_24,
        "/v1/responses",
    )
    .await;
    assert_eq!(stream.status(), StatusCode::OK);
    let body = String::from_utf8(
        to_bytes(stream.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("response.completed"), "{body}");
    assert!(body.contains("private stream"), "{body}");
    for line in body.lines().filter_map(|line| line.strip_prefix("data: ")) {
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(model) = event.pointer("/response/model")
        {
            assert_eq!(model, "gpt-6-luna");
        }
    }
    assert_eq!(private_calls.load(Ordering::SeqCst), 2);
    assert_eq!(team_calls.load(Ordering::SeqCst), 1);
    let mut custom = config(&private);
    custom.mappings.push(RequestModelMapping {
        request_model: "my-custom-luna".into(),
        target_model_id: private.clone(),
    });
    gateway
        .admin()
        .update_request_models(&private_key.id, custom)
        .await
        .unwrap();
    let response = execute_non_stream_request_with_headers(
        gateway.clone(),
        headers(&private_key),
        AiRequest::new("my-custom-luna", Vec::new()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["model"], "my-custom-luna");
    assert!(body.to_string().contains("custom reply"));
    assert_eq!(private_calls.load(Ordering::SeqCst), 3);
    gateway
        .admin()
        .update_request_models(&private_key.id, config(&private))
        .await
        .unwrap();
    let all = gateway
        .admin()
        .create_api_key(
            serde_json::from_value(serde_json::json!({"name":"All","model_ids":[]})).unwrap(),
        )
        .await
        .unwrap();
    gateway
        .admin()
        .capture_request_models(&all.id, true)
        .await
        .unwrap();
    gateway.request_model_capture.observe(
        &all.id,
        "open-responses",
        &RawEnvelope::default(),
        &AiRequest::new("gpt-6-luna", Vec::new()),
    );
    let ambiguous = gateway.admin().request_models(&all.id).await.unwrap();
    assert_eq!(ambiguous.suggestions[0].candidates.len(), 2);
    assert_eq!(ambiguous.suggestions[0].target_model_id, None);
    shutdown_test_gateway(gateway).await;
    let resumed = Gateway::new(crate::config::GatewayConfig {
        data_dir: data_dir.path().into(),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(
        resumed
            .admin()
            .request_models(&private_key.id)
            .await
            .unwrap()
            .config,
        config(&private)
    );
    assert!(
        !resumed
            .admin()
            .request_models(&all.id)
            .await
            .unwrap()
            .capture
            .active
    );
    close_test_gateway(resumed, data_dir).await;
}

#[tokio::test]
async fn request_models_do_not_expand_permissions_and_capture_unknown_authenticated_names() {
    let data_dir = tempfile::tempdir().unwrap();
    let gateway = Gateway::new(crate::config::GatewayConfig {
        data_dir: data_dir.path().into(),
        ..Default::default()
    })
    .await
    .unwrap();
    let (url, calls) = serve_sse_sequence(vec![openai_sse("authorized")]).await;
    let private = configure_route_with_id(&gateway, "private/gpt-6-luna", &[url.clone()]).await;
    let team = configure_route_with_id(&gateway, "team/gpt-6-luna", &[url]).await;
    let key = bound_key(&gateway, "Private", &private).await;
    assert!(
        gateway
            .admin()
            .update_request_models(&key.id, config(&team))
            .await
            .is_err()
    );
    gateway
        .admin()
        .capture_request_models(&key.id, true)
        .await
        .unwrap();
    let unknown = execute_non_stream_request_with_headers(
        gateway.clone(),
        headers(&key),
        AiRequest::new("gpt-6-luna", Vec::new()),
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    let view = gateway.admin().request_models(&key.id).await.unwrap();
    assert_eq!(view.capture.samples.len(), 1);
    assert_eq!(
        view.suggestions[0].target_model_id.as_deref(),
        Some(private.as_str())
    );
    // A stale/tampered binding can never skip the final Route authorization.
    gateway
        .storage
        .settings()
        .set(
            &format!("{}{}", crate::request_models::SETTING_PREFIX, key.id),
            &serde_json::to_string(&config(&team)).unwrap(),
        )
        .await
        .unwrap();
    let denied = execute_non_stream_request_with_headers(
        gateway.clone(),
        headers(&key),
        AiRequest::new("gpt-6-luna", Vec::new()),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    gateway
        .admin()
        .update_request_models(&key.id, config(&private))
        .await
        .unwrap();
    gateway
        .admin()
        .update_request_models(
            &key.id,
            RequestModelConfig {
                enabled: false,
                ..config(&private)
            },
        )
        .await
        .unwrap();
    let disabled = execute_non_stream_request_with_headers(
        gateway.clone(),
        headers(&key),
        AiRequest::new("gpt-6-luna", Vec::new()),
    )
    .await;
    assert_eq!(disabled.status(), StatusCode::NOT_FOUND);
    let canonical = execute_non_stream_request_with_headers(
        gateway.clone(),
        headers(&key),
        AiRequest::new("private/gpt-6-luna", Vec::new()),
    )
    .await;
    assert_eq!(canonical.status(), StatusCode::OK);
    to_bytes(canonical.into_body(), usize::MAX).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    gateway
        .admin()
        .clear_request_model_capture(&key.id)
        .await
        .unwrap();
    assert!(
        !gateway
            .admin()
            .request_models(&key.id)
            .await
            .unwrap()
            .capture
            .active
    );
    close_test_gateway(gateway, data_dir).await;
}
