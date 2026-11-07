use super::{
    decoder::ResponsesDecoder, encoder::ResponsesEncoder, formatter::ResponsesResponseFormatter,
    parser::ResponsesStreamParser,
};
use serde_json::{Value, json};
fn call() -> Value {
    json!({"type":"custom_tool_call","id":"ctc_1","call_id":"call_1","namespace":"functions","name":"apply_patch","input":"*** Begin Patch\n+你好\n*** End Patch\n","status":"completed"})
}
fn resource(output: Vec<Value>, status: &str) -> Value {
    super::formatter::response_resource_snapshot(
        "resp_custom",
        "gpt-test",
        status,
        output,
        Value::Null,
        Value::Null,
        Value::Null,
    )
}
#[test]
fn custom_tool_history_preserves_plain_text_and_namespace() {
    let result = json!({"type":"custom_tool_call_output","call_id":"call_1","output":[{"type":"input_text","text":"done\n"}]});
    let request = ResponsesDecoder
        .decode_request(json!({"model":"test","input":[call(),result.clone()]}))
        .unwrap();
    let (wire, _) = ResponsesEncoder.encode_request(request).unwrap();
    assert_eq!(wire["input"][0], call());
    assert_eq!(wire["input"][1], result);
}
#[test]
fn custom_tool_buffered_response_keeps_native_call() {
    let response = super::parser::ResponsesResponseParser
        .parse_response(resource(vec![call()], "completed"))
        .unwrap();
    assert_eq!(response.stop_reason.as_deref(), Some("tool_calls"));
    assert_eq!(
        ResponsesResponseFormatter.format_response(&response)["output"][0],
        call()
    );
}
fn events() -> Vec<Value> {
    let mut added = call();
    added["input"] = json!("");
    added["status"] = json!("in_progress");
    vec![
        json!({"type":"response.created","response":resource(vec![],"in_progress")}),
        json!({"type":"response.output_item.added","output_index":0,"item":added}),
        json!({"type":"response.custom_tool_call_input.delta","item_id":"ctc_1","output_index":0,"delta":"*** Begin Patch\n"}),
        json!({"type":"response.custom_tool_call_input.delta","item_id":"ctc_1","output_index":0,"delta":"+你好\n*** End Patch\n"}),
        json!({"type":"response.custom_tool_call_input.done","item_id":"ctc_1","output_index":0,"input":call()["input"]}),
        json!({"type":"response.output_item.done","output_index":0,"item":call()}),
        json!({"type":"response.completed","response":resource(vec![call()],"completed")}),
    ]
}
#[test]
fn custom_tool_streams_incrementally_over_sse_and_websocket() {
    for websocket in [false, true] {
        let mut parser = ResponsesStreamParser::new();
        let (_, mut formatter) = crate::transform::ProtocolTransform::global()
            .bind(
                stravia_runtime_contract::protocol::ids::OPEN_RESPONSES_2026_04_24,
                stravia_runtime_contract::protocol::ids::OPEN_RESPONSES_2026_04_24,
            )
            .unwrap()
            .stream()
            .unwrap()
            .into_parts();
        let mut wire = Vec::new();
        for (index, event) in events().iter().enumerate() {
            let deltas = if websocket {
                parser.parse_websocket_event(event).unwrap()
            } else {
                parser
                    .parse_chunk(&format!(
                        "event: {}\ndata: {}\n\n",
                        event["type"].as_str().unwrap(),
                        event
                    ))
                    .unwrap()
            };
            wire.extend(
                formatter
                    .encode_deltas(&deltas)
                    .unwrap()
                    .into_iter()
                    .map(|e| serde_json::from_str::<Value>(&e.data).unwrap()),
            );
            if index == 2 {
                assert!(
                    wire.iter()
                        .any(|e| e["type"] == "response.custom_tool_call_input.delta"),
                    "input must stream before completion"
                );
            }
        }
        parser.finish().unwrap();
        let terminal = wire
            .iter()
            .find(|e| e["type"] == "response.completed")
            .unwrap();
        let final_call = &terminal["response"]["output"][0];
        assert_eq!(final_call["input"], call()["input"]);
        assert_eq!(final_call["namespace"], "functions");
        assert_eq!(final_call["type"], "custom_tool_call");
        for event in [
            "response.output_item.added",
            "response.output_item.done",
            "response.custom_tool_call_input.done",
        ] {
            assert_eq!(wire.iter().filter(|e| e["type"] == event).count(), 1);
        }
        let ids: Vec<_> = wire
            .iter()
            .filter_map(|e| e.get("item_id").or_else(|| e.pointer("/item/id")))
            .collect();
        assert!(ids.iter().all(|id| *id == &final_call["id"]));
    }
}
#[test]
fn custom_tool_invalid_history_is_rejected() {
    for bad in [
        json!({"type":"custom_tool_call","call_id":"x","name":"patch","input":{}}),
        json!({"type":"custom_tool_call","call_id":"","name":"patch","input":"ok"}),
        json!({"type":"custom_tool_call_output","call_id":"x","output":42}),
    ] {
        assert!(
            ResponsesDecoder
                .decode_request(json!({"model":"test","input":[bad]}))
                .is_err()
        );
    }
}
#[test]
fn custom_tool_stream_rejects_mismatched_item_and_input() {
    for wrong_id in [false, true] {
        let mut parser = ResponsesStreamParser::new();
        let mut source = events();
        if wrong_id {
            source[2]["item_id"] = json!("other")
        } else {
            source[4]["input"] = json!("changed")
        };
        assert!(
            source
                .iter()
                .any(|e| parser.parse_websocket_event(e).is_err())
        );
    }
}

#[test]
fn custom_tool_completed_item_without_input_events_is_delivered_once() {
    let mut parser = ResponsesStreamParser::new();
    let (_, mut formatter) = crate::transform::ProtocolTransform::global()
        .bind(
            stravia_runtime_contract::protocol::ids::OPEN_RESPONSES_2026_04_24,
            stravia_runtime_contract::protocol::ids::OPEN_RESPONSES_2026_04_24,
        )
        .unwrap()
        .stream()
        .unwrap()
        .into_parts();
    let mut wire = Vec::new();
    for event in events().into_iter().filter(|e| {
        !e["type"]
            .as_str()
            .unwrap()
            .starts_with("response.custom_tool_call_input.")
    }) {
        wire.extend(
            formatter
                .encode_deltas(&parser.parse_websocket_event(&event).unwrap())
                .unwrap()
                .into_iter()
                .map(|e| serde_json::from_str::<Value>(&e.data).unwrap()),
        );
    }
    assert_eq!(
        wire.iter()
            .filter(|e| e["type"] == "response.custom_tool_call_input.done")
            .count(),
        1
    );
    assert_eq!(
        wire.iter()
            .find(|e| e["type"] == "response.completed")
            .unwrap()["response"]["output"][0]["input"],
        call()["input"]
    );
}
#[test]
fn custom_tool_completed_state_cannot_change_at_terminal() {
    let mut parser = ResponsesStreamParser::new();
    let mut source = events();
    source.last_mut().unwrap()["response"]["output"][0]["input"] = json!("changed");
    assert!(
        source
            .iter()
            .any(|e| parser.parse_websocket_event(e).is_err())
    );
}

#[test]
fn custom_tool_terminal_only_item_is_not_lost() {
    let mut parser = ResponsesStreamParser::new();
    let mut formatter = super::stream::ResponsesStreamFormatter::new();
    let mut wire = Vec::new();
    for event in [events()[0].clone(), events().last().unwrap().clone()] {
        wire.extend(
            formatter
                .format_deltas(&parser.parse_websocket_event(&event).unwrap())
                .into_iter()
                .map(|e| serde_json::from_str::<Value>(&e.data).unwrap()),
        );
    }
    assert_eq!(
        wire.iter()
            .find(|e| e["type"] == "response.completed")
            .unwrap()["response"]["output"][0]["input"],
        call()["input"]
    );
}
