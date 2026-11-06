//! Opt-in request projection for Claude Code on Antigravity Claude targets.
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
pub(crate) const OPTION: &str = "claude_code_compat";
pub(crate) const TERMS_OPTION: &str = "claude_code_compat_terms";
pub(crate) const DEFAULT_TERMS: &str = "Anthropic";
pub(crate) fn apply(
    options: &BTreeMap<String, Value>,
    logical_model: &str,
    schemas: &BTreeMap<String, Value>,
    body: &mut Value,
) -> Result<(), String> {
    if !logical_model.to_ascii_lowercase().starts_with("claude-") {
        return Ok(());
    }
    let enabled = match options.get(OPTION) {
        None => false,
        Some(Value::Bool(enabled)) => *enabled,
        Some(_) => return Err("Claude Code compatibility must be a boolean".into()),
    };
    if !enabled {
        return Ok(());
    }
    let configured = match options.get(TERMS_OPTION) {
        None => DEFAULT_TERMS,
        Some(Value::String(value)) => value,
        Some(_) => return Err("Claude Code compatibility terms must be text".into()),
    };
    if configured.len() > 2048 {
        return Err("Claude Code compatibility terms exceed 2048 bytes".into());
    }
    let mut unique = BTreeSet::new();
    let mut terms = Vec::new();
    for term in configured
        .lines()
        .map(str::trim)
        .filter(|term| !term.is_empty())
    {
        if !term.is_ascii() || !(3..=128).contains(&term.len()) {
            return Err("Each compatibility term must contain 3 to 128 ASCII characters".into());
        }
        if unique.insert(term.to_ascii_lowercase()) {
            terms.push(term);
        }
    }
    terms.sort_by_key(|term| std::cmp::Reverse(term.len()));
    if let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools {
            if let Some(functions) = tool
                .get_mut("functionDeclarations")
                .and_then(Value::as_array_mut)
            {
                for function in functions {
                    let schema = function
                        .get("name")
                        .and_then(Value::as_str)
                        .and_then(|name| schemas.get(name));
                    if let Some(schema) = schema {
                        let object = function
                            .as_object_mut()
                            .ok_or("Tool declaration must be an object")?;
                        // Keep the canonical JSON Schema exactly. The private typed
                        // Schema carrier cannot faithfully express all CC schemas.
                        object.remove("parameters");
                        object.insert("parametersJsonSchema".into(), schema.clone());
                    }
                }
            }
        }
    }
    if let Some(parts) = body
        .get_mut("systemInstruction")
        .and_then(|s| s.get_mut("parts"))
        .and_then(Value::as_array_mut)
    {
        for part in parts {
            if let Some(Value::String(text)) = part.get_mut("text") {
                for term in &terms {
                    *text = encode_literal(text, term);
                }
            }
        }
    }
    Ok(())
}
fn encode_literal(text: &str, term: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let needle = term.to_ascii_lowercase();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(offset) = lower[cursor..].find(&needle) {
        let start = cursor + offset;
        let end = start + term.len();
        // ASCII terms make these byte boundaries valid UTF-8 boundaries.
        output.push_str(&text[cursor..start + 1]);
        output.push('\u{200b}');
        output.push_str(&text[start + 1..end]);
        cursor = end;
    }
    output.push_str(&text[cursor..]);
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn enabled() -> BTreeMap<String, Value> {
        BTreeMap::from([(OPTION.into(), json!(true))])
    }
    fn schemas() -> BTreeMap<String, Value> {
        BTreeMap::from([(
            "TaskUpdate".into(),
            json!({
                "type":"object","additionalProperties":false,"properties":{
                    "answers":{"type":"object","additionalProperties":{"type":"string"}},
                    "metadata":{"type":"object","additionalProperties":{}},
                    "status":{"anyOf":[{"type":"string","enum":["pending","completed"]},{"type":"string"}]},
                    "options":{"type":"array","minItems":2,"maxItems":4,"items":{"type":["string","null"]}}
                },"required":["answers"],"default":{"additionalProperties":{"type":"application-value"}}
            }),
        )])
    }
    fn request() -> Value {
        json!({
            "systemInstruction":{"parts":[{"text":"中文 Claude Code uses Anthropic. ANTHROPIC."}]},
            "contents":[{"parts":[{"text":"Anthropic user content"},{"functionCall":{"name":"example","args":{"Anthropic":"unchanged"}}}]}],
            "tools":[{"functionDeclarations":[{"name":"TaskUpdate","description":"Keep description","parameters":{"type":"OBJECT"}}]}]
        })
    }
    #[test]
    fn disabled_and_unconfigured_are_byte_preserving() {
        let original = request();
        for options in [
            BTreeMap::new(),
            BTreeMap::from([(OPTION.into(), json!(false))]),
        ] {
            let mut body = original.clone();
            apply(&options, "claude-sonnet-4-6", &schemas(), &mut body).unwrap();
            assert_eq!(
                serde_json::to_vec(&body).unwrap(),
                serde_json::to_vec(&original).unwrap()
            );
        }
    }
    #[test]
    fn gemini_and_other_models_are_unchanged_when_enabled() {
        for model in ["gemini-3.8-flash", "gpt-oss-120b"] {
            let mut body = request();
            apply(&enabled(), model, &schemas(), &mut body).unwrap();
            assert_eq!(body, request());
        }
    }
    #[test]
    fn original_tool_schemas_constraints_and_values_are_preserved_exactly() {
        let mut body = request();
        apply(&enabled(), "claude-sonnet-4-6", &schemas(), &mut body).unwrap();
        let tool = &body["tools"][0]["functionDeclarations"][0];
        assert_eq!(tool["parametersJsonSchema"], schemas()["TaskUpdate"]);
        assert!(tool.get("parameters").is_none());
        assert_eq!(tool["description"], "Keep description");
        assert_eq!(tool["name"], "TaskUpdate");
    }
    #[test]
    fn only_system_text_is_encoded_and_all_instructions_remain() {
        let original = request();
        let mut body = original.clone();
        apply(
            &enabled(),
            "claude-opus-4-6-thinking",
            &schemas(),
            &mut body,
        )
        .unwrap();
        assert_eq!(body["contents"], original["contents"]);
        let after = body["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(
            after.replace('\u{200b}', ""),
            original["systemInstruction"]["parts"][0]["text"]
                .as_str()
                .unwrap()
        );
        assert!(after.contains("Claude Code"));
        assert!(after.contains("A\u{200b}nthropic"));
        assert!(after.contains("A\u{200b}NTHROPIC"));
    }
    #[test]
    fn projection_is_idempotent() {
        let mut body = request();
        apply(&enabled(), "claude-sonnet-4-6", &schemas(), &mut body).unwrap();
        let once = body.clone();
        apply(&enabled(), "claude-sonnet-4-6", &schemas(), &mut body).unwrap();
        assert_eq!(body, once);
    }
    #[test]
    fn empty_terms_keep_system_text_and_still_preserve_schemas() {
        let mut options = enabled();
        options.insert(TERMS_OPTION.into(), json!(""));
        let mut body = request();
        apply(&options, "claude-sonnet-4-6", &schemas(), &mut body).unwrap();
        assert_eq!(body["systemInstruction"], request()["systemInstruction"]);
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"],
            schemas()["TaskUpdate"]
        );
    }
    #[test]
    fn invalid_options_fail_explicitly_before_mutating_request() {
        let mut body = request();
        assert!(
            apply(
                &BTreeMap::from([(OPTION.into(), json!("true"))]),
                "claude-sonnet-4-6",
                &schemas(),
                &mut body
            )
            .is_err()
        );
        for term in [
            json!(42),
            json!("ab"),
            json!("中文"),
            json!("x".repeat(2049)),
        ] {
            let mut options = enabled();
            options.insert(TERMS_OPTION.into(), term);
            assert!(apply(&options, "claude-sonnet-4-6", &schemas(), &mut body).is_err());
            assert_eq!(body, request());
        }
    }
}
