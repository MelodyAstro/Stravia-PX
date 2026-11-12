//! Bounded credential-only imports. Exported networking/routing settings are never admitted.
use super::*;
use std::collections::BTreeMap;

const MAX_DOCUMENT: usize = 1024 * 1024;
const MAX_ROWS: usize = 25;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialImportInput {
    pub kind: String,
    pub json: String,
    #[serde(default)]
    pub index: usize,
    pub name: Option<String>,
    #[serde(default)]
    pub use_proxy: bool,
    #[serde(default)]
    pub confirmed: bool,
}

#[derive(Serialize)]
pub struct CredentialImportRowView {
    pub index: usize,
    pub label: String,
    pub code: String,
    pub has_refresh_token: bool,
}

#[derive(Serialize)]
pub struct CredentialImportPreview {
    pub rows: Vec<CredentialImportRowView>,
}

struct ImportedRow {
    label: String,
    values: BTreeMap<String, Value>,
}

fn profile(kind: &str) -> anyhow::Result<(&'static str, &'static str, &'static str)> {
    match kind {
        "codex" => Ok((
            "openai-codex",
            "codex",
            "https://chatgpt.com/backend-api/codex",
        )),
        "antigravity" => Ok((
            "antigravity",
            "oauth",
            "https://daily-cloudcode-pa.googleapis.com",
        )),
        _ => anyhow::bail!("IMPORT_UNSUPPORTED_KIND"),
    }
}

fn document(input: &CredentialImportInput) -> anyhow::Result<Vec<Value>> {
    profile(&input.kind)?;
    anyhow::ensure!(
        input.json.len() <= MAX_DOCUMENT,
        "IMPORT_DOCUMENT_TOO_LARGE"
    );
    let text = input.json.trim().trim_start_matches('\u{feff}');
    anyhow::ensure!(!text.is_empty(), "IMPORT_INVALID_JSON");
    let value: Value = if input.kind == "antigravity"
        && !(text.starts_with('{') || text.starts_with('[') || text.starts_with('"'))
    {
        Value::String(text.to_owned())
    } else {
        serde_json::from_str(text).map_err(|_| anyhow::anyhow!("IMPORT_INVALID_JSON"))?
    };
    let rows = match value {
        Value::Array(rows) => rows,
        Value::Object(mut object) => {
            let wrapped = object.remove("accounts").or_else(|| {
                object
                    .get_mut("data")
                    .and_then(|v| v.as_object_mut())
                    .and_then(|v| v.remove("accounts"))
            });
            match wrapped {
                Some(Value::Array(rows)) => rows,
                Some(_) => anyhow::bail!("IMPORT_INVALID_JSON"),
                None => vec![Value::Object(object)],
            }
        }
        value @ Value::String(_) if input.kind == "antigravity" => vec![value],
        _ => anyhow::bail!("IMPORT_INVALID_JSON"),
    };
    anyhow::ensure!(
        !rows.is_empty() && rows.len() <= MAX_ROWS,
        "IMPORT_ROW_LIMIT"
    );
    Ok(rows)
}

fn optional_string(value: &Value, key: &str, max: usize) -> anyhow::Result<Option<String>> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value))
            if value.len() <= max && !value.chars().any(char::is_control) =>
        {
            Ok((!value.trim().is_empty()).then(|| value.trim().to_owned()))
        }
        _ => anyhow::bail!("IMPORT_INVALID_FIELD"),
    }
}

fn parse_row(kind: &str, value: &Value, index: usize) -> anyhow::Result<ImportedRow> {
    if let Value::String(refresh) = value {
        anyhow::ensure!(
            kind == "antigravity"
                && !refresh.is_empty()
                && refresh.len() <= 32768
                && !refresh.chars().any(char::is_whitespace),
            "IMPORT_INVALID_FIELD"
        );
        return Ok(ImportedRow {
            label: format!("Antigravity #{}", index + 1),
            values: BTreeMap::from([("refresh_token".into(), Value::String(refresh.clone()))]),
        });
    }
    anyhow::ensure!(value.is_object(), "IMPORT_INVALID_FIELD");
    let credentials = value
        .get("credentials")
        .or_else(|| value.get("tokens"))
        .unwrap_or(value);
    anyhow::ensure!(credentials.is_object(), "IMPORT_INVALID_FIELD");
    for object in [value, credentials] {
        anyhow::ensure!(
            object.get("agent_identity").is_none_or(Value::is_null)
                && object.get("agent_private_key").is_none()
                && optional_string(object, "auth_mode", 64)?
                    .is_none_or(|v| matches!(v.as_str(), "chatgpt" | "oauth")),
            "IMPORT_UNSUPPORTED_AUTH_MODE"
        );
    }
    if let Some(platform) = optional_string(value, "platform", 64)? {
        anyhow::ensure!(
            platform
                == if kind == "codex" {
                    "openai"
                } else {
                    "antigravity"
                }
                || (kind == "antigravity" && platform == "google"),
            "IMPORT_WRONG_PLATFORM"
        );
    }
    if let Some(mode) = optional_string(value, "type", 64)? {
        anyhow::ensure!(
            matches!(
                (kind, mode.as_str()),
                ("codex", "codex" | "oauth")
                    | (
                        "antigravity",
                        "antigravity" | "google" | "oauth" | "authorized_user"
                    )
            ),
            "IMPORT_UNSUPPORTED_AUTH_MODE"
        );
    }
    anyhow::ensure!(
        optional_string(value, "OPENAI_API_KEY", 32768)?.is_none()
            && optional_string(value, "personal_access_token", 32768)?.is_none(),
        "IMPORT_UNSUPPORTED_AUTH_MODE"
    );
    let mut values = BTreeMap::new();
    let keys: &[&str] = if kind == "codex" {
        &[
            "access_token",
            "refresh_token",
            "id_token",
            "account_id",
            "client_id",
            "email",
        ]
    } else {
        &[
            "access_token",
            "refresh_token",
            "project_id",
            "account_id",
            "client_id",
            "client_secret",
            "email",
        ]
    };
    for key in keys {
        if let Some(text) =
            optional_string(credentials, key, 32768)?.or(optional_string(value, key, 32768)?)
        {
            values.insert((*key).into(), Value::String(text));
        }
    }
    for (from, to) in [("chatgpt_account_id", "account_id"), ("mail", "email")] {
        if let Some(text) =
            optional_string(credentials, from, 256)?.or(optional_string(value, from, 256)?)
        {
            values.entry(to.into()).or_insert(Value::String(text));
        }
    }
    anyhow::ensure!(
        values.contains_key("refresh_token")
            || (kind == "codex" && values.contains_key("access_token")),
        "IMPORT_MISSING_TOKEN"
    );
    let label = optional_string(value, "account_name", 160)?
        .or(optional_string(value, "name", 160)?)
        .or_else(|| {
            values
                .get("email")
                .and_then(Value::as_str)
                .filter(|v| v.len() <= 160)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| {
            format!(
                "{} #{}",
                if kind == "codex" {
                    "Codex"
                } else {
                    "Antigravity"
                },
                index + 1
            )
        });
    Ok(ImportedRow { label, values })
}

impl AdminService {
    async fn duplicate_import(
        &self,
        vendor: &str,
        values: &BTreeMap<String, Value>,
    ) -> anyhow::Result<bool> {
        for provider in self.list_providers().await? {
            if provider.vendor.as_deref() != Some(vendor) {
                continue;
            }
            let Some(saved) = self
                .gw
                .storage
                .oauth_credentials()
                .get(&provider.id)
                .await?
            else {
                continue;
            };
            let meta: Value = serde_json::from_str(&saved.meta)?;
            let same_account = values
                .get("account_id")
                .and_then(Value::as_str)
                .is_some_and(|id| meta.get("account_id").and_then(Value::as_str) == Some(id));
            let same_refresh = values
                .get("refresh_token")
                .and_then(Value::as_str)
                .is_some_and(|token| saved.refresh_token.as_deref() == Some(token));
            if same_account || same_refresh {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn preview_credential_import(
        &self,
        input: CredentialImportInput,
    ) -> anyhow::Result<CredentialImportPreview> {
        let (vendor, _, _) = profile(&input.kind)?;
        let mut rows = Vec::new();
        for (index, value) in document(&input)?.iter().enumerate() {
            match parse_row(&input.kind, value, index) {
                Ok(row) => rows.push(CredentialImportRowView {
                    index,
                    label: row.label,
                    code: if self.duplicate_import(vendor, &row.values).await? {
                        "IMPORT_DUPLICATE"
                    } else {
                        "IMPORT_READY"
                    }
                    .into(),
                    has_refresh_token: row.values.contains_key("refresh_token"),
                }),
                Err(error) => rows.push(CredentialImportRowView {
                    index,
                    label: format!("#{}", index + 1),
                    code: error.to_string(),
                    has_refresh_token: false,
                }),
            }
        }
        Ok(CredentialImportPreview { rows })
    }

    pub async fn import_credential(
        &self,
        input: CredentialImportInput,
    ) -> anyhow::Result<Provider> {
        anyhow::ensure!(input.confirmed, "IMPORT_CONFIRMATION_REQUIRED");
        let admin = self.clone();
        // A disconnected browser must not cancel persistence after an upstream
        // refresh-token rotation. Retrying begins with duplicate detection.
        tokio::spawn(async move { admin.import_credential_record(input).await })
            .await
            .map_err(|_| anyhow::anyhow!("IMPORT_PROVISION_FAILED"))?
    }

    async fn import_credential_record(
        &self,
        input: CredentialImportInput,
    ) -> anyhow::Result<Provider> {
        static IMPORT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _guard = IMPORT_LOCK.lock().await;
        let (vendor, channel, base_url) = profile(&input.kind)?;
        let rows = document(&input)?;
        let row = parse_row(
            &input.kind,
            rows.get(input.index)
                .ok_or_else(|| anyhow::anyhow!("IMPORT_INVALID_INDEX"))?,
            input.index,
        )?;
        anyhow::ensure!(
            !self.duplicate_import(vendor, &row.values).await?,
            "IMPORT_DUPLICATE"
        );
        let name = input.name.unwrap_or(row.label);
        anyhow::ensure!(
            !name.trim().is_empty() && name.len() <= 160 && !name.chars().any(char::is_control),
            "IMPORT_INVALID_NAME"
        );
        let candidate = AuthSessionCandidate {
            vendor_id: vendor.into(),
            channel: channel.into(),
            provider_id: None,
            base_url: base_url.into(),
            protocol: None,
            options: Default::default(),
            credentials: Default::default(),
            use_proxy: input.use_proxy,
        };
        let (mut snapshot, descriptor) = self.provider_auth_candidate_snapshot(&candidate).await?;
        snapshot
            .operation_metadata
            .insert("use_proxy".into(), Value::Bool(input.use_proxy));
        let scope = self.gw.create_vendor_session_scope(vendor, snapshot)?;
        let execution = self
            .gw
            .execute_vendor_session(
                &scope,
                crate::plugin::VendorRequest::Auth(stravia_vendor_sdk::AuthRequest {
                    step: stravia_vendor_sdk::AuthStep::ManualInput {
                        value: serde_json::to_string(&row.values)?,
                    },
                }),
                crate::plugin::VendorCallContext::new(
                    stravia_runtime_contract::CancellationToken::new(),
                    stravia_runtime_contract::Deadline::fixed(
                        std::time::Instant::now() + std::time::Duration::from_secs(120),
                    ),
                ),
            )
            .await
            .map_err(|_| anyhow::anyhow!("IMPORT_UPSTREAM_REJECTED"))?;
        let fence = execution.publication.write_fence().await?;
        let stravia_vendor_sdk::OperationOutput::Auth(
            response @ stravia_vendor_sdk::AuthResponse::Credentials { .. },
        ) = execution.output
        else {
            anyhow::bail!("IMPORT_UPSTREAM_REJECTED");
        };
        if let stravia_vendor_sdk::AuthResponse::Credentials { values, .. } = &response {
            anyhow::ensure!(
                !self.duplicate_import(vendor, values).await?,
                "IMPORT_DUPLICATE"
            );
        }
        let session = self
            .create_auth_session_record(
                candidate,
                descriptor,
                stravia_runtime_contract::identifier::new_id(),
                Some(response),
                scope,
                OAuthSessionStartOptions {
                    callback_mode: OAuthCallbackMode::Manual,
                    redirect_uri: String::new(),
                    listener_port: None,
                    fallback_reason: None,
                },
            )
            .await?;
        *session
            .vendor_runtime
            .as_ref()
            .expect("import session runtime")
            .publication
            .lock()
            .await = Some(execution.publication.clone());
        drop(fence);
        let result = self
            .create_provider_with_oauth_session(
                &session.id,
                CreateProvider {
                    name: Some(name),
                    source: ProviderSourceInput::Custom {
                        vendor: vendor.into(),
                        channel: channel.into(),
                        protocol: None,
                        base_url: base_url.into(),
                        models_source: None,
                        static_models: None,
                    },
                    credential: ProviderCredentialInput::None,
                    vendor_options: Default::default(),
                    use_proxy: input.use_proxy,
                },
            )
            .await;
        if result.is_err() {
            self.delete_auth_session_record(&session.id).await?;
        }
        result.map_err(|_| anyhow::anyhow!("IMPORT_PROVISION_FAILED"))
    }

    pub async fn export_antigravity_rt(
        &self,
        provider_id: &str,
        confirmed: bool,
    ) -> anyhow::Result<Value> {
        anyhow::ensure!(confirmed, "IMPORT_CONFIRMATION_REQUIRED");
        let provider = self.get_provider(provider_id).await?;
        anyhow::ensure!(
            provider.vendor.as_deref() == Some("antigravity"),
            "IMPORT_UNSUPPORTED_KIND"
        );
        let saved = self
            .gw
            .storage
            .oauth_credentials()
            .get(provider_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("IMPORT_MISSING_TOKEN"))?;
        anyhow::ensure!(saved.status != "refreshing", "IMPORT_REFRESH_IN_PROGRESS");
        let refresh = saved
            .refresh_token
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow::anyhow!("IMPORT_MISSING_TOKEN"))?;
        let meta: Value = serde_json::from_str(&saved.meta)?;
        let mut exported = serde_json::json!({"type":"antigravity", "refresh_token": refresh});
        for key in [
            "email",
            "project_id",
            "account_id",
            "client_id",
            "client_secret",
        ] {
            if let Some(value) = meta.get(key).filter(|v| v.is_string()) {
                exported[key] = value.clone();
            }
        }
        Ok(exported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(kind: &str, value: Value) -> CredentialImportInput {
        CredentialImportInput {
            kind: kind.into(),
            json: value.to_string(),
            index: 0,
            name: None,
            use_proxy: false,
            confirmed: false,
        }
    }
    #[test]
    fn oauth_formats_normalize_without_network_or_exported_settings() -> anyhow::Result<()> {
        for value in [
            serde_json::json!({"OPENAI_API_KEY":null,"tokens":{"access_token":"fixture-at","refresh_token":"fixture-rt","account_id":"team-a"}}),
            serde_json::json!({"type":"codex","access_token":"fixture-at","refresh_token":"fixture-rt","account_id":"team-a"}),
            serde_json::json!({"platform":"openai","type":"oauth","credentials":{"access_token":"fixture-at","refresh_token":"fixture-rt","chatgpt_account_id":"team-a"},"proxy":"https://untrusted.invalid","base_url":"https://untrusted.invalid"}),
        ] {
            let row = parse_row("codex", &value, 0)?;
            assert_eq!(row.values["account_id"], "team-a");
            assert_eq!(row.values["refresh_token"], "fixture-rt");
            assert!(!row.values.contains_key("proxy"));
            assert!(!row.values.contains_key("base_url"));
        }
        assert_eq!(document(&input("codex", serde_json::json!({"type":"sub2api-data","version":1,"accounts":[{"credentials":{"refresh_token":"fixture-rt"}}]})))?.len(), 1);
        assert_eq!(parse_row("antigravity", &serde_json::json!({"mail":"fixture@example.invalid","refresh_token":"fixture-rt","project_id":"project"}), 0)?.values["email"], "fixture@example.invalid");
        Ok(())
    }
    #[test]
    fn credential_import_all_four_codex_documents_preserve_credentials_only() -> anyhow::Result<()> {
        let flat = serde_json::json!({
            "type":"codex", "id_token":"fixture-id", "access_token":"fixture-at",
            "refresh_token":"fixture-rt", "account_id":"fixture-team",
            "email":"fixture@example.invalid", "last_refresh":"2026-01-01T00:00:00Z",
            "expired":"2026-01-02T00:00:00Z"
        });
        let official = serde_json::json!({"auth_mode":"chatgpt", "OPENAI_API_KEY":null,
            "tokens":flat, "last_refresh":"2026-01-01T00:00:00Z"});
        let sub2api = serde_json::json!({"type":"sub2api-data", "version":1, "accounts":[{
            "name":"Fixture", "platform":"openai", "type":"oauth", "credentials":flat,
            "proxy":"https://untrusted.invalid", "base_url":"https://untrusted.invalid"
        }]});
        for (format, document_value) in [
            ("official auth.json", official),
            ("Cockpit Tools array", serde_json::json!([flat.clone()])),
            ("CLIProxyAPI record", flat.clone()),
            ("sub2api accounts", sub2api),
        ] {
            let request = input("codex", document_value);
            let rows = document(&request)?;
            assert_eq!(rows.len(), 1, "{format}");
            let row = parse_row("codex", &rows[0], 0)?;
            assert_eq!(row.values["id_token"], "fixture-id", "{format}");
            assert_eq!(row.values["access_token"], "fixture-at", "{format}");
            assert_eq!(row.values["refresh_token"], "fixture-rt", "{format}");
            assert_eq!(row.values["account_id"], "fixture-team", "{format}");
            assert_eq!(row.values.len(), 5, "{format}");
        }
        let mut escaped = input("codex", Value::Null);
        escaped.json = "[\\\n&#x20; {\\\n\"refresh_token\":\"private-fixture\"\\\n}]".into();
        let error = document(&escaped).unwrap_err().to_string();
        assert_eq!(error, "IMPORT_INVALID_JSON");
        Ok(())
    }

    #[test]
    fn malformed_and_non_oauth_material_is_rejected_without_echoing_it() {
        for value in [
            serde_json::json!({"auth_mode":"agentIdentity","agent_private_key":"sensitive-fixture"}),
            serde_json::json!({"OPENAI_API_KEY":"sensitive-fixture"}),
            serde_json::json!({"refresh_token":123}),
            serde_json::json!({"platform":"anthropic","refresh_token":"sensitive-fixture"}),
            serde_json::json!({"refresh_token":"bad\ntoken"}),
        ] {
            let error = parse_row("codex", &value, 0)
                .err()
                .expect("reject invalid row")
                .to_string();
            assert!(error.starts_with("IMPORT_"));
            assert!(!error.contains("sensitive-fixture"));
        }
        assert!(document(&input("codex", serde_json::json!(vec![Value::Null; 26]))).is_err());
        assert!(document(&input("codex", serde_json::json!("fixture-rt"))).is_err());
    }

    #[tokio::test]
    async fn credential_import_preview_deduplicates_and_rt_backup_round_trips_without_access_token()
    -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let gateway = Gateway::from_storage(
            crate::config::GatewayConfig {
                data_dir: directory.path().into(),
                ..Default::default()
            },
            std::sync::Arc::new(crate::storage::MemoryStorage::new(
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )),
        )
        .await?;
        let provider = gateway
            .storage
            .providers()
            .create(CreateProviderRecord {
                name: "Existing Google account".into(),
                vendor: Some("antigravity".into()),
                channel: Some("oauth".into()),
                protocol: "google-gemini-generate-content-v1beta".into(),
                base_url: "https://daily-cloudcode-pa.googleapis.com".into(),
                preset_key: None,
                models_source: None,
                static_models: None,
                api_key: String::new(),
                adapter_credentials: "{}".into(),
                vendor_options: "{}".into(),
                auth_mode: "oauth".into(),
                use_proxy: false,
            })
            .await?;
        gateway.storage.oauth_credentials().upsert(&provider.id, UpsertOAuthCredential { driver_key: "antigravity".into(), scheme: "oauth_auth_code_pkce".into(), access_token: "private-access-fixture".into(), refresh_token: Some("private-refresh-fixture".into()), expires_at: None, resource_url: None, subject_id: None, scopes: None, meta: Some(serde_json::json!({"email":"fixture@example.invalid", "project_id":"project", "account_id":"google-account", "unrelated":"private-note-fixture"}).to_string()) }).await?;
        let admin = gateway.admin();
        assert!(
            admin
                .export_antigravity_rt(&provider.id, false)
                .await
                .is_err()
        );
        let exported = admin.export_antigravity_rt(&provider.id, true).await?;
        assert_eq!(exported["refresh_token"], "private-refresh-fixture");
        assert!(!exported.to_string().contains("private-access-fixture"));
        assert!(!exported.to_string().contains("private-note-fixture"));
        let preview = admin
            .preview_credential_import(input("antigravity", exported))
            .await?;
        assert_eq!(preview.rows[0].code, "IMPORT_DUPLICATE");
        assert!(!serde_json::to_string(&preview)?.contains("private-refresh-fixture"));
        assert_eq!(admin.list_providers().await?.len(), 1);
        Ok(())
    }
}
