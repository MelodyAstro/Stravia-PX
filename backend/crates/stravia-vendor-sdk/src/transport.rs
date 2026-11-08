//! Provider transport options shared by generated forms and the native host.
use crate::{ConfigField, ConfigFieldKind, EnumOption, LocalizedText};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const CIPHER_SUITES: &[&str] = &[
    "TLS13_AES_256_GCM_SHA384",
    "TLS13_AES_128_GCM_SHA256",
    "TLS13_CHACHA20_POLY1305_SHA256",
    "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
    "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256",
    "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256",
    "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
    "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256",
    "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
];
pub const KEY_EXCHANGE_GROUPS: &[&str] = &["X25519MLKEM768", "X25519", "secp256r1", "secp384r1"];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportProfile {
    pub min_version: Option<String>,
    pub max_version: Option<String>,
    pub cipher_suites: Vec<String>,
    pub key_exchange_groups: Vec<String>,
    pub http1_only: bool,
}

impl TransportProfile {
    pub fn from_options(options: &BTreeMap<String, Value>) -> Result<Self, String> {
        let get = |key: &str| -> Result<Option<String>, String> {
            options
                .get(key)
                .map(|v| {
                    v.as_str()
                        .map(|s| s.trim().to_owned())
                        .ok_or_else(|| format!("{key} must be a string"))
                })
                .transpose()
                .map(|s| s.filter(|s| !s.is_empty() && s != "default"))
        };
        let min_version = get("tls_min_version")?;
        let max_version = get("tls_max_version")?;
        if min_version
            .as_deref()
            .into_iter()
            .chain(max_version.as_deref())
            .any(|s| s != "1.2" && s != "1.3")
        {
            return Err("TLS versions must be 1.2 or 1.3".into());
        }
        if min_version.as_deref().unwrap_or("1.2") > max_version.as_deref().unwrap_or("1.3") {
            return Err("Minimum TLS version exceeds maximum TLS version".into());
        }
        let list = |key: &str, allowed: &[&str]| -> Result<Vec<String>, String> {
            let value = get(key)?.unwrap_or_default();
            if value.len() > 2048 {
                return Err(format!("{key} is too long"));
            }
            let mut seen = BTreeSet::new();
            value
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| {
                    if !allowed.contains(&v) || !seen.insert(v) {
                        return Err(format!("{key} contains an unsupported or duplicate value"));
                    }
                    Ok(v.to_owned())
                })
                .collect()
        };
        let cipher_suites = list("tls_cipher_suites", CIPHER_SUITES)?;
        let key_exchange_groups = list("tls_key_exchange_groups", KEY_EXCHANGE_GROUPS)?;
        if min_version.as_deref().unwrap_or("1.2") == "1.2"
            && !key_exchange_groups.is_empty()
            && key_exchange_groups.iter().all(|g| g == "X25519MLKEM768")
        {
            return Err("TLS 1.2 requires a non-hybrid key exchange group".into());
        }
        if !cipher_suites.is_empty() {
            for version in ["1.2", "1.3"] {
                if version >= min_version.as_deref().unwrap_or("1.2")
                    && version <= max_version.as_deref().unwrap_or("1.3")
                    && !cipher_suites
                        .iter()
                        .any(|s| s.starts_with("TLS13_") == (version == "1.3"))
                {
                    return Err("Cipher suites must cover each enabled TLS version".into());
                }
            }
        }
        let http = get("http_version")?;
        if http
            .as_deref()
            .is_some_and(|v| !matches!(v, "auto" | "http1"))
        {
            return Err("HTTP version must be auto or http1".into());
        }
        Ok(Self {
            min_version,
            max_version,
            cipher_suites,
            key_exchange_groups,
            http1_only: http.as_deref() == Some("http1"),
        })
    }

    pub fn has_custom_tls(&self) -> bool {
        self.min_version.is_some()
            || self.max_version.is_some()
            || !self.cipher_suites.is_empty()
            || !self.key_exchange_groups.is_empty()
    }
}

fn text(en: &str, zh: &str) -> LocalizedText {
    LocalizedText::from_translations([("en-US", en), ("zh-CN", zh)])
}

pub fn config_fields() -> Vec<ConfigField> {
    let mut fields = Vec::new();
    for (key, en, zh, description, choices) in [
        (
            "tls_min_version",
            "Minimum TLS version",
            "最低 TLS 版本",
            (
                "Minimum outbound TLS version. Applies to HTTPS and WSS.",
                "出站 TLS 最低版本，适用于 HTTPS 和 WSS。",
            ),
            vec![
                ("default", "Default", "默认"),
                ("1.2", "TLS 1.2", "TLS 1.2"),
                ("1.3", "TLS 1.3", "TLS 1.3"),
            ],
        ),
        (
            "tls_max_version",
            "Maximum TLS version",
            "最高 TLS 版本",
            ("Maximum outbound TLS version.", "出站 TLS 最高版本。"),
            vec![
                ("default", "Default", "默认"),
                ("1.2", "TLS 1.2", "TLS 1.2"),
                ("1.3", "TLS 1.3", "TLS 1.3"),
            ],
        ),
        (
            "http_version",
            "Upstream HTTP protocol",
            "上游 HTTP 协议",
            (
                "HTTP negotiates HTTP/2 when available. WebSocket always uses HTTP/1.1.",
                "自动模式允许 HTTP/2；WebSocket 始终使用 HTTP/1.1。",
            ),
            vec![
                ("auto", "Automatic", "自动"),
                ("http1", "HTTP/1.1", "HTTP/1.1"),
            ],
        ),
    ] {
        fields.push(ConfigField {
            key: key.into(),
            label: text(en, zh),
            description: Some(text(description.0, description.1)),
            kind: ConfigFieldKind::Enum {
                options: choices
                    .iter()
                    .map(|(value, en, zh)| EnumOption {
                        value: (*value).into(),
                        label: text(en, zh),
                    })
                    .collect(),
            },
            required: false,
            default_json: Some(Value::String(choices[0].0.into())),
            group: None,
            secret: false,
            min: None,
            max: None,
            max_length: None,
            pattern: None,
            visible_when: None,
        });
    }
    for (key, en, zh, description) in [
        (
            "tls_cipher_suites",
            "TLS cipher suites (ordered)",
            "TLS 加密套件（有序）",
            (
                "Comma-separated Rustls cipher names. Blank keeps defaults; include a suite for every enabled TLS version.",
                "逗号分隔的 Rustls 套件名称，按填写顺序发送；留空使用默认值，须覆盖每个启用的 TLS 版本。",
            ),
        ),
        (
            "tls_key_exchange_groups",
            "TLS key exchange groups (ordered)",
            "TLS 密钥交换组（有序）",
            (
                "Comma-separated: X25519MLKEM768, X25519, secp256r1, secp384r1. Blank keeps defaults. TLS extension order remains Rustls-owned.",
                "逗号分隔：X25519MLKEM768、X25519、secp256r1、secp384r1；留空使用默认值。TLS 扩展顺序由 Rustls 管理。",
            ),
        ),
    ] {
        fields.push(ConfigField {
            key: key.into(),
            label: text(en, zh),
            description: Some(text(description.0, description.1)),
            kind: ConfigFieldKind::String { multiline: true },
            required: false,
            default_json: None,
            group: None,
            secret: false,
            min: None,
            max: None,
            max_length: Some(2048),
            pattern: None,
            visible_when: None,
        });
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn options(value: Value) -> BTreeMap<String, Value> {
        serde_json::from_value(value).unwrap()
    }
    #[test]
    fn tls_profile_rejects_incompatible_versions_and_suites() {
        for input in [
            json!({"tls_min_version":"1.3","tls_max_version":"1.2"}),
            json!({"tls_min_version":"1.0"}),
            json!({"tls_cipher_suites":"TLS13_AES_128_GCM_SHA256"}),
            json!({"tls_key_exchange_groups":"X25519,X25519"}),
            json!({"tls_key_exchange_groups":"X25519MLKEM768"}),
            json!({"http_version":"h3"}),
        ] {
            assert!(TransportProfile::from_options(&options(input)).is_err());
        }
        let profile = TransportProfile::from_options(&options(json!({"tls_min_version":"1.3","tls_cipher_suites":"TLS13_CHACHA20_POLY1305_SHA256,TLS13_AES_128_GCM_SHA256","tls_key_exchange_groups":"X25519,secp256r1"}))).unwrap();
        assert_eq!(profile.cipher_suites[0], "TLS13_CHACHA20_POLY1305_SHA256");
        assert_eq!(profile.key_exchange_groups, ["X25519", "secp256r1"]);
    }
}
