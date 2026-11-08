//! Guest SDK for independently installable Stravia platform extensions.
//!
//! Build a `cdylib` for `wasm32-wasip2`, implement [`PlatformPlugin`] and call
//! [`export_plugin!`]. The resulting component needs no access to the host's
//! filesystem, network, environment or provider credentials.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const API_VERSION: u32 = 1;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    Request,
    UpstreamResponse,
    ToolResult,
    ClientOutput,
}

impl HookKind {
    pub const fn operation(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::UpstreamResponse => "upstream_response",
            Self::ToolResult => "tool_result",
            Self::ClientOutput => "client_output",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ToolManifest {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(default)]
    pub parallel_safe: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    #[serde(default)]
    pub hooks: Vec<HookKind>,
    #[serde(default)]
    pub tools: Vec<ToolManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<Value>,
}

impl PluginManifest {
    /// Validate identities and declarations before any component is installed.
    pub fn validate(&self) -> Result<(), String> {
        if self.api_version != API_VERSION {
            return Err(format!(
                "unsupported platform plugin API version: {}",
                self.api_version
            ));
        }
        if !identifier(&self.id, 80) || self.id.starts_with('.') || self.id.ends_with('.') {
            return Err(
                "plugin id must contain 1–80 ASCII letters, digits, dots, hyphens or underscores"
                    .into(),
            );
        }
        if self.name.trim().is_empty()
            || self.name.len() > 256
            || self.name.chars().any(char::is_control)
        {
            return Err("plugin name must be nonempty plain text of at most 256 bytes".into());
        }
        semver::Version::parse(&self.version)
            .map_err(|_| "plugin version must be semantic versioning".to_string())?;
        if self.hooks.len() > 4
            || self.hooks.iter().collect::<HashSet<_>>().len() != self.hooks.len()
        {
            return Err("plugin hook declarations must be unique".into());
        }
        if self.tools.len() > 32 {
            return Err("a plugin may declare at most 32 tools".into());
        }
        let mut names = HashSet::new();
        for tool in &self.tools {
            if !identifier(&tool.name, 64) || !names.insert(&tool.name) {
                return Err(
                    "plugin tool names must be unique ASCII identifiers of at most 64 bytes".into(),
                );
            }
            if tool.description.trim().is_empty()
                || tool.description.len() > 4096
                || tool.description.chars().any(char::is_control)
            {
                return Err(
                    "tool descriptions must be nonempty plain text of at most 4096 bytes".into(),
                );
            }
            if !tool.parameters.is_object()
                || tool.parameters.get("type").and_then(Value::as_str) != Some("object")
            {
                return Err("tool parameters must be an object JSON Schema".into());
            }
        }
        if self.hooks.is_empty() && self.tools.is_empty() {
            return Err("a plugin must declare at least one hook or tool".into());
        }
        if self
            .config_schema
            .as_ref()
            .is_some_and(|schema| !schema.is_object())
        {
            return Err("plugin configuration schema must be a JSON object".into());
        }
        Ok(())
    }

    pub fn supports(&self, operation: &str) -> bool {
        self.hooks.iter().any(|hook| hook.operation() == operation)
            || operation
                .strip_prefix("tool:")
                .is_some_and(|name| self.tools.iter().any(|tool| tool.name == name))
    }
}

fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

pub trait PlatformPlugin {
    fn manifest() -> PluginManifest;
    fn invoke(operation: &str, input: Value) -> Result<Value, String>;
}

#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        world: "platform-plugin",
        path: "wit",
        generate_all,
        pub_export_macro: true,
    });
}

#[doc(hidden)]
pub fn describe_json<G: PlatformPlugin>() -> Result<String, String> {
    let manifest = G::manifest();
    manifest.validate()?;
    let output = serde_json::to_string(&manifest).map_err(|error| error.to_string())?;
    if output.len() > MAX_MANIFEST_BYTES {
        return Err("plugin manifest exceeds 64 KiB".into());
    }
    Ok(output)
}

#[doc(hidden)]
pub fn invoke_json<G: PlatformPlugin>(operation: String, input: String) -> Result<String, String> {
    if input.len() > MAX_MESSAGE_BYTES {
        return Err("plugin input exceeds 4 MiB".into());
    }
    let value = serde_json::from_str(&input).map_err(|error| error.to_string())?;
    let output =
        serde_json::to_string(&G::invoke(&operation, value)?).map_err(|error| error.to_string())?;
    if output.len() > MAX_MESSAGE_BYTES {
        return Err("plugin output exceeds 4 MiB".into());
    }
    Ok(output)
}

#[macro_export]
macro_rules! export_plugin {
    ($plugin:ty) => {
        struct __StraviaPlatformPlugin;
        impl $crate::bindings::Guest for __StraviaPlatformPlugin {
            fn describe() -> Result<String, String> {
                $crate::describe_json::<$plugin>()
            }
            fn invoke(operation: String, input: String) -> Result<String, String> {
                $crate::invoke_json::<$plugin>(operation, input)
            }
        }
        $crate::bindings::export!(__StraviaPlatformPlugin with_types_in $crate::bindings);
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest() -> PluginManifest {
        PluginManifest {
            id: "example.policy".into(),
            name: "Policy".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            hooks: vec![HookKind::Request],
            tools: vec![],
            config_schema: None,
        }
    }

    #[test]
    fn rejects_unsupported_versions_and_duplicate_capabilities() {
        let mut value = manifest();
        assert!(value.validate().is_ok());
        value.api_version = 2;
        assert!(value.validate().is_err());
        value.api_version = API_VERSION;
        value.hooks.push(HookKind::Request);
        assert!(value.validate().is_err());
    }

    #[test]
    fn rejects_unsafe_identity_and_tool_schema() {
        let mut value = manifest();
        value.id = "../escape".into();
        assert!(value.validate().is_err());
        value.id = "example.policy".into();
        value.tools.push(ToolManifest {
            name: "echo".into(),
            description: "Echo input".into(),
            parameters: json!({"type":"array"}),
            parallel_safe: true,
        });
        assert!(value.validate().is_err());
        value.tools[0].parameters = json!({"type":"object"});
        assert!(value.validate().is_ok());
        assert!(value.supports("request"));
        assert!(value.supports("tool:echo"));
        assert!(!value.supports("tool:other"));
    }
}
