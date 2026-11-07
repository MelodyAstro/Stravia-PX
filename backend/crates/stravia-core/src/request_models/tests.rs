use super::*;
use stravia_runtime_contract::protocol::ir::{AiItem, MessageContent, Role};

fn mapping(name: &str, target: &str) -> RequestModelMapping {
    RequestModelMapping {
        request_model: name.into(),
        target_model_id: target.into(),
    }
}

#[test]
fn request_models_are_exact_single_hop_and_disabled_by_default() {
    let mut config = RequestModelConfig {
        enabled: false,
        mappings: vec![
            mapping("gpt-6-luna", "private-key"),
            mapping("private-key", "team-key"),
        ],
    };
    assert_eq!(config.target("gpt-6-luna"), None);
    config.enabled = true;
    assert_eq!(config.target("gpt-6-luna"), Some("private-key"));
    assert_eq!(config.target("gpt-6-luna-latest"), None);
    assert_eq!(config.target("GPT-6-LUNA"), None);
    assert_eq!(config.target(" gpt-6-luna"), None);
}

#[test]
fn request_models_reject_duplicates_invalid_names_and_unbounded_config() {
    let mut config = RequestModelConfig {
        enabled: true,
        mappings: vec![mapping("luna", "a"), mapping("luna", "b")],
    };
    assert!(config.validate().is_err());
    for invalid in ["", "a b", "a\nb", "模型", "*"] {
        config.mappings = vec![mapping(invalid, "a")];
        assert!(config.validate().is_err());
    }
    config.mappings = vec![mapping("custom:gpt_6.1-test", "a")];
    assert!(config.validate().is_ok());
    config.mappings = (0..129)
        .map(|i| mapping(&format!("model-{i}"), "a"))
        .collect();
    assert!(config.validate().is_err());
    assert!(
        serde_json::from_str::<RequestModelConfig>(r#"{"enabled":true,"mapings":[]}"#).is_err()
    );
}

#[test]
fn request_model_capture_is_private_bounded_and_principal_scoped() {
    let store = CaptureStore::default();
    store.set("private", true).unwrap();
    let request = AiRequest::new(
        "gpt-6-luna",
        vec![AiItem {
            role: Role::User,
            content: MessageContent::Text("PRIVATE-CHAT-TEXT".to_owned().into()),
            tool_calls: None,
            tool_call_id: None,
            meta: None,
        }],
    );
    let envelope = RawEnvelope::new(
        Some(serde_json::json!({"secret":"RAW-BODY-SECRET"})),
        HashMap::from([("authorization".into(), "Bearer HEADER-SECRET".into())]),
        "POST",
        "/v1/responses",
    );
    store.observe("other", "open-responses", &envelope, &request);
    assert!(store.view("private").samples.is_empty());
    store.observe("private", "open-responses", &envelope, &request);
    store.observe("private", "open-responses", &envelope, &request);
    let view = store.view("private");
    assert_eq!(view.samples.len(), 1);
    assert_eq!(view.samples[0].count, 2);
    let serialized = serde_json::to_string(&view).unwrap();
    for secret in [
        "PRIVATE-CHAT-TEXT",
        "RAW-BODY-SECRET",
        "HEADER-SECRET",
        "authorization",
    ] {
        assert!(!serialized.contains(secret));
    }
    for i in 0..25 {
        store.observe(
            "private",
            "open-responses",
            &envelope,
            &AiRequest::new(format!("model-{i}"), Vec::new()),
        );
    }
    assert_eq!(store.view("private").samples.len(), MAX_SAMPLES);
    assert!(store.view("private").dropped_samples > 0);
    store.set("private", false).unwrap();
    store.observe("private", "open-responses", &envelope, &request);
    assert_eq!(store.view("private").samples[0].count, 2);
    assert!(!store.view("private").active);
    store.clear("private");
    assert!(store.view("private").samples.is_empty());
}

#[test]
fn request_model_capture_expires_without_incoming_traffic_and_clears_results() {
    let store = CaptureStore::default();
    store.set("key", true).unwrap();
    store.0.lock().get_mut("key").unwrap().deadline = Instant::now() - Duration::from_secs(1);
    assert!(!store.view("key").active);
    store.0.lock().get_mut("key").unwrap().remove_at = Instant::now() - Duration::from_secs(1);
    assert!(store.view("key").expires_at.is_none());
}

#[test]
fn request_model_capture_caps_concurrent_sessions() {
    let store = CaptureStore::default();
    for i in 0..16 {
        store.set(&format!("key-{i}"), true).unwrap();
    }
    assert!(store.set("overflow", true).is_err());
    store.clear("key-0");
    assert!(store.set("overflow", true).is_ok());
}

#[test]
fn request_model_capture_counts_positional_tools_without_retaining_schema() {
    let store = CaptureStore::default();
    store.set("key", true).unwrap();
    let request = AiRequest::new(
        "gpt-6-luna",
        vec![AiItem::unknown(serde_json::json!({
            "type":"additional_tools","role":"developer","tools":[{"type":"namespace","name":"functions","tools":[
                {"type":"function","name":"a","parameters":{"private-note":"SCHEMA-MUST-NOT-BE-RETAINED"}},
                {"type":"function","name":"b"}
            ]}]
        }))],
    );
    let envelope = RawEnvelope::new(
        None,
        HashMap::from([("upgrade".into(), "websocket".into())]),
        "GET",
        "/v1/responses",
    );
    store.observe("key", "open-responses", &envelope, &request);
    let view = store.view("key");
    assert_eq!(view.samples[0].tools, 2);
    assert_eq!(view.samples[0].transport, "websocket");
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains("SCHEMA-MUST-NOT-BE-RETAINED")
    );
}
