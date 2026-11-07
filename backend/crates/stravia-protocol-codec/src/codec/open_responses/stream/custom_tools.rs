use super::{PendingOutputItem, ResponsesStreamFormatter, gateway_item_id};
use crate::SseEvent;
use serde_json::{Value, json};

impl ResponsesStreamFormatter {
    pub(super) fn format_custom_tool_event(
        &mut self,
        events: &mut Vec<SseEvent>,
        mut event: Value,
    ) {
        let Some(index) = event
            .get("output_index")
            .and_then(Value::as_u64)
            .and_then(|i| usize::try_from(i).ok())
        else {
            return;
        };
        if event["type"] == "response.output_item.added" {
            self.start_custom_tool(events, index, &event["item"]);
            return;
        }
        let Some(pending) = self
            .standalone_items
            .iter_mut()
            .find(|p| p.output_index == index)
        else {
            return;
        };
        let kind = event["type"].as_str().unwrap_or_default().to_owned();
        let done = kind.ends_with(".done");
        if done {
            let completed = event["input"].as_str().unwrap_or_default();
            let previous = pending.item["input"].as_str().unwrap_or_default();
            if let Some(missing) = completed.strip_prefix(previous).filter(|s| !s.is_empty()) {
                events.push(SseEvent::new(Some("response.custom_tool_call_input.delta"), json!({"type":"response.custom_tool_call_input.delta","output_index":index,"item_id":pending.item["id"],"delta":missing}).to_string()));
            }
            pending.item["input"] = event["input"].clone();
            self.custom_input_done.insert(index);
        } else {
            let mut input = pending.item["input"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            input.push_str(event["delta"].as_str().unwrap_or_default());
            pending.item["input"] = Value::String(input);
        }
        event["item_id"] = pending.item["id"].clone();
        events.push(SseEvent::new(Some(&kind), event.to_string()));
    }

    fn start_custom_tool(&mut self, events: &mut Vec<SseEvent>, index: usize, raw: &Value) {
        if self
            .standalone_items
            .iter()
            .any(|p| p.output_index == index)
        {
            return;
        }
        self.finish_unindexed_message(events);
        self.ensure_started(events);
        self.seal_reasoning_item(events);
        self.next_output_index = self.next_output_index.max(index + 1);
        let mut item = raw.clone();
        item["id"] = Value::String(gateway_item_id("item", &self.resp_id, index));
        item["status"] = json!("in_progress");
        if item["type"] == "custom_tool_call" {
            item["input"] = json!("");
        }
        events.push(SseEvent::new(
            Some("response.output_item.added"),
            json!({"type":"response.output_item.added","output_index":index,"item":item})
                .to_string(),
        ));
        self.standalone_items.push(PendingOutputItem {
            output_index: index,
            item,
            done: false,
        });
    }

    pub(super) fn finish_custom_tool(
        &mut self,
        events: &mut Vec<SseEvent>,
        index: usize,
        raw: &Value,
    ) {
        self.start_custom_tool(events, index, raw);
        if raw["type"] == "custom_tool_call" && !self.custom_input_done.contains(&index) {
            self.format_custom_tool_event(events, json!({"type":"response.custom_tool_call_input.done","output_index":index,"input":raw["input"]}));
        }
        let pending = self
            .standalone_items
            .iter_mut()
            .find(|p| p.output_index == index)
            .expect("custom item started");
        if pending.done {
            return;
        }
        let id = pending.item["id"].clone();
        pending.item = raw.clone();
        pending.item["id"] = id;
        pending.done = true;
        events.push(SseEvent::new(
            Some("response.output_item.done"),
            json!({"type":"response.output_item.done","output_index":index,"item":pending.item})
                .to_string(),
        ));
    }
}
