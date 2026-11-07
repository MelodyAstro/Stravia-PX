use anyhow::{Context, ensure};
use serde_json::Value;

// Installation and execution must never resolve an untrusted schema URL.
fn local_references(value: &Value) -> anyhow::Result<()> {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(key.as_str(), "$ref" | "$dynamicRef" | "$recursiveRef") {
                    ensure!(
                        value.as_str().is_some_and(|value| value.starts_with('#')),
                        "plugin schemas may only reference local definitions"
                    );
                }
                if key == "$schema" {
                    // Select a fixed dialect locally; a guest cannot add a custom resolver.
                    ensure!(
                        value.as_str().is_some_and(|value| matches!(
                            value,
                            "https://json-schema.org/draft/2020-12/schema"
                                | "https://json-schema.org/draft/2019-09/schema"
                                | "http://json-schema.org/draft-07/schema#"
                                | "http://json-schema.org/draft-06/schema#"
                                | "http://json-schema.org/draft-04/schema#"
                        )),
                        "unsupported plugin schema dialect"
                    );
                }
                local_references(value)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                local_references(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn compile(schema: &Value) -> anyhow::Result<jsonschema::Validator> {
    local_references(schema)?;
    jsonschema::validator_for(schema).context("invalid plugin JSON schema")
}

pub(super) fn validate(schema: &Value, value: &Value) -> anyhow::Result<()> {
    compile(schema)?
        .validate(value)
        .map_err(|error| anyhow::anyhow!("plugin value does not match its schema: {error}"))
}

pub(super) fn configuration(manifest: &super::PluginManifest, value: &Value) -> anyhow::Result<()> {
    ensure!(value.is_object(), "plugin configuration must be an object");
    ensure!(
        serde_json::to_vec(value)?.len() <= 256 * 1024,
        "plugin configuration exceeds 256 KiB"
    );
    if let Some(schema) = &manifest.config_schema {
        validate(schema, value)?;
    }
    Ok(())
}
