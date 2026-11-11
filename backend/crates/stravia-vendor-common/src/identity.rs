//! Configured or explicitly enabled client-supplied outbound identity.
use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use stravia_vendor_sdk::{
    ConfigField, ConfigFieldKind, GuestHost, LocalizedText, PluginError, ProviderSnapshot,
    read_http_body,
};

use crate::common::plugin_error;

const STATE_KEY: &str = "client_identity_version_v1";
const SYNC_INTERVAL_MS: i64 = 60 * 60 * 1000;

pub fn text<'a>(options: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a str> {
    options
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub fn version<'a>(provider: &'a ProviderSnapshot, fallback: &'a str) -> &'a str {
    if client_mode(provider) {
        if let Some(value) =
            client_header(provider, "version").filter(|v| stable_version(v).is_some())
        {
            return value;
        }
        if let Some(value) = client_header(provider, "user-agent")
            .and_then(|ua| ua.split_once('/'))
            .map(|(_, v)| v.split_whitespace().next().unwrap_or(""))
            .filter(|v| stable_version(v).is_some())
        {
            return value;
        }
    }
    text(&provider.options, "client_version").unwrap_or(fallback)
}

pub fn user_agent(provider: &ProviderSnapshot, fallback: &str, version: &str) -> String {
    if client_mode(provider) {
        if let Some(ua) = client_header(provider, "user-agent") {
            return ua.to_owned();
        }
    }
    text(&provider.options, "user_agent")
        .unwrap_or(fallback)
        .replace("{version}", version)
}

pub fn field(key: &str, en: &str, zh: &str, description: (&str, &str), limit: u32) -> ConfigField {
    ConfigField {
        key: key.into(),
        label: localized(en, zh),
        description: Some(localized(description.0, description.1)),
        kind: ConfigFieldKind::String { multiline: false },
        required: false,
        default_json: None,
        group: None,
        secret: false,
        min: None,
        max: None,
        max_length: Some(limit),
        pattern: None,
        visible_when: None,
    }
}

fn localized(en: &str, zh: &str) -> LocalizedText {
    LocalizedText::from_translations([("en-US", en), ("zh-CN", zh)])
}

pub fn fields(sync: bool) -> Vec<ConfigField> {
    let mut fields = vec![field(
        "user_agent",
        "User-Agent",
        "User-Agent",
        (
            "Outbound User-Agent. Use {version} for the selected client version; blank uses the provider default.",
            "发送给上游的 UA；{version} 随有效客户端版本替换，留空使用提供商默认值。",
        ),
        1024,
    )];
    fields.push(ConfigField {
        key: "identity_source".into(), label: localized("Client identity source", "客户端身份来源"),
        description: Some(localized("Configured values or request-scoped client identity headers. Credentials and session IDs remain host-owned; TLS uses a separate outbound preset.", "使用云端配置，或按请求透传客户端身份头；凭据和会话标识仍由平台管理，TLS 另选出站预设。")),
        kind: ConfigFieldKind::Enum { options: [("configured", "Cloud configuration", "云端配置"), ("client", "Request client", "请求客户端")].into_iter().map(|(value,en,zh)| stravia_vendor_sdk::EnumOption { value: value.into(), label: localized(en,zh) }).collect() },
        required: false, default_json: Some(json!("configured")), group: None, secret: false, min: None, max: None, max_length: None, pattern: None, visible_when: None,
    });
    if sync {
        let mut version = field(
            "client_version",
            "Client version",
            "客户端版本",
            (
                "Optional stable version pin. Takes precedence over official version synchronization.",
                "可选的稳定版本固定值，优先于官方版本同步。",
            ),
            32,
        );
        version.pattern = Some(r"^([0-9]+\.[0-9]+\.[0-9]+)?$".into());
        fields.push(version);
        let mut sync = field(
            "auto_sync_client_version",
            "Sync official client version",
            "同步官方客户端版本",
            (
                "Refresh stable releases during allowance/model checks, at most hourly. Model calls use the last good version without a release lookup. SDK and TLS settings remain explicit.",
                "额度/模型检查时刷新官方稳定版，每小时最多一次；推理直接使用已缓存版本。SDK 和 TLS 设置需单独配置。",
            ),
            0,
        );
        sync.kind = ConfigFieldKind::Bool;
        sync.max_length = None;
        sync.default_json = Some(json!(false));
        fields.push(sync);
    }
    fields.extend(stravia_vendor_sdk::transport::config_fields());
    fields
}

/// Apply only explicit software/environment overrides, never credentials or turn IDs.
pub fn apply_claude(
    provider: &ProviderSnapshot,
    headers: &mut Vec<(String, String)>,
    fallback: &str,
) {
    set_header(
        headers,
        "user-agent",
        user_agent(
            provider,
            "claude-cli/{version} (external, cli)",
            version(provider, fallback),
        ),
    );
    for (option, name) in [
        ("package_version", "x-stainless-package-version"),
        ("runtime_version", "x-stainless-runtime-version"),
        ("runtime", "x-stainless-runtime"),
        ("os", "x-stainless-os"),
        ("arch", "x-stainless-arch"),
        ("timeout", "x-stainless-timeout"),
        ("anthropic_version", "anthropic-version"),
    ] {
        if let Some(value) = text(&provider.options, option) {
            set_header(headers, name, value.into());
        }
    }
    apply_client_headers(provider, headers);
}

pub fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    headers.retain(|(key, _)| !key.eq_ignore_ascii_case(name));
    headers.push((name.into(), value));
}

fn stable_version(raw: &str) -> Option<[u64; 3]> {
    let values = raw
        .split('.')
        .map(|s| {
            (!s.is_empty()
                && s.len() <= 10
                && s.bytes().all(|b| b.is_ascii_digit())
                && (s.len() == 1 || !s.starts_with('0')))
            .then(|| s.parse().ok())
            .flatten()
        })
        .collect::<Option<Vec<_>>>()?;
    values.try_into().ok()
}

pub fn validate(options: &BTreeMap<String, Value>) -> Result<(), PluginError> {
    if options
        .get("identity_source")
        .is_some_and(|v| !matches!(v.as_str(), Some("configured" | "client")))
    {
        return Err(invalid("Identity source must be configured or client"));
    }
    if options
        .get("client_version")
        .is_some_and(|v| !v.is_string())
    {
        return Err(invalid("Client version must be a string"));
    }
    if options
        .get("auto_sync_client_version")
        .is_some_and(|v| !v.is_boolean())
    {
        return Err(invalid(
            "Official client version synchronization must be a boolean",
        ));
    }
    if let Some(value) = text(options, "client_version")
        && (value.len() > 32 || stable_version(value).is_none())
    {
        return Err(invalid(
            "Client version must be a stable major.minor.patch version",
        ));
    }
    for key in [
        "user_agent",
        "originator",
        "package_version",
        "runtime_version",
        "runtime",
        "os",
        "arch",
        "timeout",
        "anthropic_version",
    ] {
        if let Some(value) = options.get(key) {
            let value = value
                .as_str()
                .ok_or_else(|| invalid("Identity values must be strings"))?;
            if value.len() > 1024 || value.bytes().any(|b| b.is_ascii_control() || !b.is_ascii()) {
                return Err(invalid(
                    "Identity headers must contain printable ASCII only",
                ));
            }
        }
    }
    stravia_vendor_sdk::transport::TransportProfile::from_options(options).map_err(invalid)?;
    Ok(())
}

pub fn validation_issues(
    options: &BTreeMap<String, Value>,
) -> Vec<stravia_vendor_sdk::ValidationIssue> {
    validate(options)
        .err()
        .map(|error| stravia_vendor_sdk::ValidationIssue {
            field: None,
            code: "invalid_client_identity".into(),
            message: LocalizedText::english(error.message),
        })
        .into_iter()
        .collect()
}

/// Refresh only on auxiliary checks, keeping GitHub off the model request path.
/// The namespaced JSON section preserves OAuth and native continuation state.
pub fn prepare(
    host: &GuestHost,
    provider: &mut ProviderSnapshot,
    repo: &str,
    prefix: &str,
    fallback: &str,
    refresh: bool,
) -> Result<(), PluginError> {
    validate(&provider.options)?;
    if client_mode(provider)
        || text(&provider.options, "client_version").is_some()
        || provider
            .options
            .get("auto_sync_client_version")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Ok(());
    }
    let root = read_root(host)?;
    let mut state = root
        .get(STATE_KEY)
        .filter(|s| s["repo"].as_str() == Some(repo))
        .cloned()
        .unwrap_or_else(|| json!({"repo":repo}));
    let now = chrono::Utc::now().timestamp_millis();
    let checked = state["checked_at_ms"].as_i64().unwrap_or(0);
    if refresh && (checked == 0 || now < checked || now.saturating_sub(checked) >= SYNC_INTERVAL_MS)
    {
        state["checked_at_ms"] = json!(now);
        match fetch_version(host, repo, prefix) {
            Ok(latest) => {
                let current = state["version"]
                    .as_str()
                    .and_then(stable_version)
                    .unwrap_or([0, 0, 0]);
                if stable_version(&latest).is_some_and(|v| v >= current) {
                    state["version"] = json!(latest);
                }
                state["synced_at_ms"] = json!(now);
                state
                    .as_object_mut()
                    .expect("identity state object")
                    .remove("last_error");
            }
            Err(_) => {
                host.log(
                    stravia_vendor_sdk::LogLevel::Warn,
                    "Official client version refresh failed; retaining last good version",
                );
                state["last_error"] = json!("release_lookup_failed");
            }
        }
        // Network I/O can overlap another operation. Merge into the latest root,
        // preserving native sessions and any newer successful synchronization.
        let mut root = read_root(host)?;
        if let Some(current) = root
            .get(STATE_KEY)
            .filter(|s| s["repo"].as_str() == Some(repo))
            && current["version"].as_str().and_then(stable_version)
                > state["version"].as_str().and_then(stable_version)
        {
            state["version"] = current["version"].clone();
        }
        root.insert(STATE_KEY.into(), state.clone());
        host.write_private_state(
            &serde_json::to_vec(&root)
                .map_err(|_| invalid("Client identity state could not be encoded"))?,
        )?;
    }
    if let Some(synced) = state["version"]
        .as_str()
        .filter(|v| stable_version(v).is_some_and(|v| Some(v) >= stable_version(fallback)))
    {
        provider
            .options
            .insert("client_version".into(), json!(synced));
    }
    Ok(())
}

fn read_root(host: &GuestHost) -> Result<Map<String, Value>, PluginError> {
    match host.read_private_state()? {
        Some(bytes) if !bytes.is_empty() => {
            serde_json::from_slice(&bytes).map_err(|_| invalid("Client identity state is invalid"))
        }
        _ => Ok(Map::new()),
    }
}

fn fetch_version(host: &GuestHost, repo: &str, prefix: &str) -> Result<String, PluginError> {
    for suffix in ["releases/latest", "releases?per_page=30"] {
        let response = host.http_start(stravia_vendor_sdk::HttpRequest {
            method: "GET".into(),
            url: format!("https://api.github.com/repos/{repo}/{suffix}"),
            // No provider/client credentials ever reach the release service.
            headers: vec![
                ("accept".into(), "application/vnd.github+json".into()),
                ("user-agent".into(), "Stravia-client-version-sync".into()),
            ],
            body: Vec::new(),
        })?;
        let status = response.status()?;
        if status == 404 {
            continue;
        }
        if status != 200 {
            return Err(invalid("Official release service failed"));
        }
        let body = read_http_body(&response, 2 * 1024 * 1024)?;
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| invalid("Invalid official release response"))?;
        if let Some(version) = latest_stable(&value, prefix) {
            return Ok(version);
        }
    }
    Err(invalid("No official stable release found"))
}

fn latest_stable(value: &Value, prefix: &str) -> Option<String> {
    let releases = value
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| std::slice::from_ref(value));
    releases
        .iter()
        .filter(|r| r["draft"].as_bool() == Some(false) && r["prerelease"].as_bool() == Some(false))
        .filter_map(|r| r["tag_name"].as_str()?.strip_prefix(prefix))
        .filter_map(|v| Some((stable_version(v)?, v)))
        .max_by_key(|(v, _)| *v)
        .map(|(_, v)| v.to_owned())
}

fn invalid(message: impl Into<String>) -> PluginError {
    plugin_error(stravia_vendor_sdk::ErrorKind::Invalid, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_identity_is_opt_in_request_scoped_and_cannot_replace_credentials() {
        let mut provider: ProviderSnapshot = serde_json::from_value(json!({
            "provider_id":"claude-code", "channel":"oauth", "base_url":"https://example.test", "protocol":"anthropic-messages",
            "options":{"client_version":"2.1.319"}, "client_headers":[
                ["User-Agent","claude-cli/2.2.1 (external, cli)"],
                ["x-stainless-runtime-version","v26.12.1"],
                ["authorization","Bearer caller-secret"], ["cookie","caller-cookie"], ["session-id","caller-session"]
            ]
        })).unwrap();
        assert_eq!(version(&provider, "2.0.0"), "2.1.319");
        let mut headers = vec![("authorization".into(), "Bearer provider-secret".into())];
        apply_claude(&provider, &mut headers, "2.0.0");
        assert!(!headers.iter().any(|(_, value)| value.contains("2.2.1")));
        provider
            .options
            .insert("identity_source".into(), json!("client"));
        let options = provider.options.clone();
        assert_eq!(version(&provider, "2.0.0"), "2.2.1");
        apply_claude(&provider, &mut headers, "2.0.0");
        let values: BTreeMap<_, _> = headers.into_iter().collect();
        assert_eq!(values["user-agent"], "claude-cli/2.2.1 (external, cli)");
        assert_eq!(values["authorization"], "Bearer provider-secret");
        assert!(!values.contains_key("cookie"));
        assert!(!values.contains_key("session-id"));
        assert_eq!(provider.options, options);
    }
    #[test]
    fn official_versions_exclude_prereleases_and_compare_numerically() {
        let values = json!([
            {"tag_name":"rust-v0.9.0","draft":false,"prerelease":false},
            {"tag_name":"rust-v0.12.0","draft":false,"prerelease":false},
            {"tag_name":"rust-v9.0.0-beta","draft":false,"prerelease":false},
            {"tag_name":"rust-v9.0.0","draft":true,"prerelease":false},
            {"tag_name":"rust-v8.0.0","draft":false,"prerelease":true}
        ]);
        assert_eq!(latest_stable(&values, "rust-v").as_deref(), Some("0.12.0"));
        assert!(latest_stable(&values, "v").is_none());
        assert!(stable_version("01.2.3").is_none());
    }
    #[test]
    fn manual_identity_pin_wins_without_reading_or_refreshing_state() {
        let mut provider = ProviderSnapshot {
            provider_id: "claude-code".into(),
            channel: "oauth".into(),
            base_url: "https://example.test".into(),
            protocol: "anthropic-messages".into(),
            options: BTreeMap::from([
                ("client_version".into(), json!("2.3.4")),
                ("auto_sync_client_version".into(), json!(true)),
                (
                    "user_agent".into(),
                    json!("claude-cli/{version} (external, cli)"),
                ),
                ("package_version".into(), json!("0.128.0")),
            ]),
            credentials: Default::default(),
            model: None,
            model_metadata: None,
            client_headers: vec![("user-agent".into(), "caller-ua".into())],
            operation_metadata: Default::default(),
        };
        // A host import in a native test panics, proving a fixed pin needs no imports.
        prepare(
            &GuestHost::new(),
            &mut provider,
            "anthropics/claude-code",
            "v",
            "2.1.317",
            true,
        )
        .unwrap();
        let mut headers = vec![
            ("authorization".into(), "Bearer provider-secret".into()),
            ("user-agent".into(), "previous".into()),
        ];
        apply_claude(&provider, &mut headers, "2.1.317");
        let values: BTreeMap<_, _> = headers.into_iter().collect();
        assert_eq!(values["user-agent"], "claude-cli/2.3.4 (external, cli)");
        assert_eq!(values["x-stainless-package-version"], "0.128.0");
        assert_eq!(values["authorization"], "Bearer provider-secret");
    }

    #[test]
    fn identity_headers_reject_injection_and_invalid_versions() {
        for (key, value) in [
            ("user_agent", "x\r\nAuthorization: secret"),
            ("client_version", "1.2.3-beta"),
        ] {
            assert!(validate(&BTreeMap::from([(key.into(), json!(value))])).is_err());
        }
        for (key, value) in [
            ("client_version", json!(123)),
            ("auto_sync_client_version", json!("true")),
        ] {
            assert!(validate(&BTreeMap::from([(key.into(), value)])).is_err());
        }
    }
}

pub fn client_mode(provider: &ProviderSnapshot) -> bool {
    text(&provider.options, "identity_source") == Some("client")
}

pub fn client_header<'a>(provider: &'a ProviderSnapshot, name: &str) -> Option<&'a str> {
    let mut values = provider
        .client_headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name));
    let (_, value) = values.next()?;
    // Ambiguous or malformed input never becomes an upstream identity.
    (values.next().is_none()
        && !value.is_empty()
        && value.len() <= 1024
        && value.bytes().all(|b| b.is_ascii() && !b.is_ascii_control()))
    .then_some(value.as_str())
}

pub fn client_header_names() -> std::collections::BTreeSet<String> {
    [
        "user-agent",
        "version",
        "originator",
        "x-stainless-lang",
        "x-stainless-package-version",
        "x-stainless-os",
        "x-stainless-arch",
        "x-stainless-runtime",
        "x-stainless-runtime-version",
        "x-stainless-timeout",
        "anthropic-version",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// Only software identity fields cross this boundary, never auth, cookies,
/// routing, forwarded IPs or session/thread identifiers. No shared state is written.
pub fn apply_client_headers(provider: &ProviderSnapshot, headers: &mut Vec<(String, String)>) {
    if !client_mode(provider) {
        return;
    }
    for name in client_header_names() {
        if let Some(value) = client_header(provider, &name) {
            set_header(headers, &name, value.into());
        }
    }
}
