//! Header-declared client characteristics, never proof of an official client.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientProfile {
    pub family: String,
    pub version: Option<String>,
    pub package_version: Option<String>,
    pub runtime_version: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub suggested_http_shape: Option<String>,
    pub suggested_ws_shape: Option<String>,
}

impl ClientProfile {
    pub fn from_headers<'a>(headers: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let headers = headers.into_iter().collect::<Vec<_>>();
        let header = |name: &str| -> Option<&str> {
            let mut matching = headers
                .iter()
                .filter(|(key, _)| key.eq_ignore_ascii_case(name));
            let (_, value) = matching.next()?;
            (matching.next().is_none()
                && value.len() <= 1024
                && value.bytes().all(|b| b.is_ascii() && !b.is_ascii_control()))
            .then_some(*value)
        };
        let ua = header("user-agent").unwrap_or("");
        let product = ua.split('/').next().unwrap_or("").to_ascii_lowercase();
        let (family, http, ws) = match product.as_str() {
            "claude-cli" | "claude-code" => {
                ("claude_code", Some("claude-code"), Some("claude-code"))
            }
            "codex_cli_rs" | "codex-tui" | "codex" => {
                ("codex", Some("codex-http"), Some("codex-ws"))
            }
            "antigravity" => ("antigravity", Some("antigravity"), Some("antigravity")),
            _ => ("unknown", None, None),
        };
        let ua_version = ua
            .split_once('/')
            .and_then(|(_, rest)| rest.split_whitespace().next());
        let version = if family == "unknown" {
            None
        } else {
            header("version")
                .filter(|v| stable_version(v))
                .or_else(|| ua_version.filter(|v| stable_version(v)))
                .map(str::to_owned)
        };
        let component = |name| {
            header(name)
                .filter(|v| {
                    v.len() <= 64
                        && !v.is_empty()
                        && v.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                })
                .map(str::to_owned)
        };
        Self {
            family: family.into(),
            version,
            package_version: component("x-stainless-package-version"),
            runtime_version: component("x-stainless-runtime-version"),
            os: component("x-stainless-os"),
            arch: component("x-stainless-arch"),
            suggested_http_shape: http.map(str::to_owned),
            suggested_ws_shape: ws.map(str::to_owned),
        }
    }
}

fn stable_version(value: &str) -> bool {
    value.len() <= 32
        && semver::Version::parse(value)
            .is_ok_and(|version| version.pre.is_empty() && version.build.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_declared_clients_without_retaining_raw_headers() {
        let profile = ClientProfile::from_headers([
            ("User-Agent", "claude-cli/2.1.317 (external, cli)"),
            ("authorization", "PRIVATE-TOKEN"),
            ("x-stainless-runtime-version", "v24.3.0"),
        ]);
        assert_eq!(profile.family, "claude_code");
        assert_eq!(profile.version.as_deref(), Some("2.1.317"));
        assert_eq!(profile.runtime_version.as_deref(), Some("v24.3.0"));
        assert!(
            !serde_json::to_string(&profile)
                .unwrap()
                .contains("PRIVATE-TOKEN")
        );
        let codex =
            ClientProfile::from_headers([("user-agent", "codex_cli_rs/0.167.0 (Linux; x86_64)")]);
        assert_eq!(codex.suggested_ws_shape.as_deref(), Some("codex-ws"));
        assert!(
            ClientProfile::from_headers([("user-agent", "claude-cli/999.0.0-local")])
                .version
                .is_none()
        );
        assert_eq!(
            ClientProfile::from_headers([("user-agent", "custom/1.0.0")]).family,
            "unknown"
        );
        assert_eq!(
            ClientProfile::from_headers([
                ("user-agent", "claude-cli/2.1.317"),
                ("user-agent", "codex/0.167.0")
            ])
            .family,
            "unknown"
        );
    }
}
