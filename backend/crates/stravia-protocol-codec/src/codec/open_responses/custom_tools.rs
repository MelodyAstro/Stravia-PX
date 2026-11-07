//! Native free-form tool calls stay distinct from JSON function calls.
use anyhow::Result;
use serde_json::Value;

pub(crate) fn is_custom_tool_item(kind: &str) -> bool {
    matches!(kind, "custom_tool_call" | "custom_tool_call_output")
}

pub(super) fn validate_item(item: &Value) -> Result<()> {
    let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
    if !is_custom_tool_item(kind) {
        anyhow::bail!("invalid custom tool item type");
    }
    for field in if kind == "custom_tool_call" {
        &["call_id", "name"][..]
    } else {
        &["call_id"][..]
    } {
        if item
            .get(*field)
            .and_then(Value::as_str)
            .is_none_or(|s| s.trim().is_empty())
        {
            anyhow::bail!("{kind} requires non-empty {field}");
        }
    }
    if kind == "custom_tool_call" {
        if !item.get("input").is_some_and(Value::is_string) {
            anyhow::bail!("custom_tool_call input must be a string");
        }
    } else {
        // Reuse the supported text/image/file output validation without changing wire bytes.
        let mut output = item.clone();
        output["type"] = Value::String("function_call_output".into());
        super::decoder::decode_input_item(&output)?;
    }
    for field in ["id", "namespace"] {
        if let Some(value) = item.get(field).filter(|v| !v.is_null())
            && value.as_str().is_none_or(str::is_empty)
        {
            anyhow::bail!("{kind} {field} must be a non-empty string or null");
        }
    }
    if let Some(status) = item.get("status")
        && !matches!(
            status.as_str(),
            Some("in_progress" | "completed" | "incomplete" | "failed")
        )
    {
        anyhow::bail!("{kind} has invalid status");
    }
    Ok(())
}

pub(crate) fn is_stream_event(event: &Value) -> bool {
    match event.get("type").and_then(Value::as_str) {
        Some("response.custom_tool_call_input.delta" | "response.custom_tool_call_input.done") => {
            true
        }
        Some("response.output_item.added") => {
            event.pointer("/item/type").and_then(Value::as_str) == Some("custom_tool_call")
        }
        _ => false,
    }
}
