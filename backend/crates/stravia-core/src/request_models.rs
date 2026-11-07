//! Per-principal client model names. Route identity and authorization stay canonical.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, ensure};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use stravia_runtime_contract::Principal;
use stravia_runtime_contract::protocol::ir::{AiRequest, RawEnvelope};

use crate::Gateway;
use crate::db::models::RouteConfig;

pub(crate) const SETTING_PREFIX: &str = "px_request_models:";
const MAX_MAPPINGS: usize = 128;
const MAX_SAMPLES: usize = 20;
const CAPTURE_SECONDS: u64 = 180;
const RESULT_SECONDS: u64 = 900;

fn invalid(code: &str, message: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        serde_json::json!({"code":format!("REQUEST_MODELS_{code}"),"message":message,"params":{}})
    )
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RequestModelConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mappings: Vec<RequestModelMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RequestModelMapping {
    pub request_model: String,
    /// Stable Route key, never another alias or a provider model name.
    pub target_model_id: String,
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/:".contains(&b))
}

impl RequestModelConfig {
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.mappings.len() <= MAX_MAPPINGS,
            invalid("LIMIT", "At most 128 model mappings are allowed")
        );
        let mut names = HashSet::new();
        for mapping in &self.mappings {
            ensure!(
                valid_name(&mapping.request_model),
                invalid(
                    "NAME",
                    "Request model names must contain 1–160 ASCII letters, digits or . _ - / :"
                )
            );
            ensure!(
                !mapping.target_model_id.is_empty() && mapping.target_model_id.len() <= 160,
                invalid("TARGET_REQUIRED", "A target model is required")
            );
            ensure!(
                names.insert(&mapping.request_model),
                invalid("DUPLICATE", "Duplicate request model name")
            );
        }
        Ok(())
    }

    pub(crate) fn target<'a>(&'a self, requested: &str) -> Option<&'a str> {
        self.enabled
            .then(|| self.mappings.iter().find(|m| m.request_model == requested))
            .flatten()
            .map(|m| m.target_model_id.as_str())
    }
}

pub(crate) async fn load(gateway: &Gateway, key_id: &str) -> anyhow::Result<RequestModelConfig> {
    let value = gateway
        .storage
        .settings()
        .get(&format!("{SETTING_PREFIX}{key_id}"))
        .await?;
    let config: RequestModelConfig = value
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .context("Invalid request model mapping configuration")?
        .unwrap_or_default();
    config.validate()?;
    Ok(config)
}

/// No request/body mutation: response model fields and continuation identity retain
/// the client's model name. The selected Route still goes through Security.
pub(crate) async fn resolve(
    gateway: &Gateway,
    principal: &Principal,
    requested: &str,
) -> anyhow::Result<Option<RouteConfig>> {
    Ok(resolve_mapping(gateway, principal, requested).await?.0)
}

pub(crate) async fn resolve_mapping(
    gateway: &Gateway,
    principal: &Principal,
    requested: &str,
) -> anyhow::Result<(Option<RouteConfig>, bool)> {
    let config = load(gateway, principal.api_key_id()).await?;
    let cache = gateway.model_cache.read().await;
    let mapped = config.target(requested);
    let route = match mapped {
        Some(target) => cache
            .models
            .iter()
            .find(|model| model.id.as_str() == target)
            .cloned(),
        None => cache.resolve(requested).cloned(),
    };
    Ok((route, mapped.is_some()))
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestModelTarget {
    pub id: String,
    pub model_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RequestModelSample {
    pub request_model: String,
    pub protocol: String,
    pub transport: String,
    pub stream: bool,
    pub input_items: usize,
    pub tools: usize,
    pub count: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RequestModelCapture {
    pub active: bool,
    pub expires_at: Option<String>,
    pub samples: Vec<RequestModelSample>,
    /// Additional distinct shapes were not retained; never silently truncate.
    pub dropped_samples: u32,
}

struct Session {
    deadline: Instant,
    remove_at: Instant,
    view: RequestModelCapture,
}

#[derive(Clone, Default)]
pub(crate) struct CaptureStore(Arc<Mutex<HashMap<String, Session>>>);

impl CaptureStore {
    fn view(&self, key_id: &str) -> RequestModelCapture {
        let mut sessions = self.0.lock();
        let now = Instant::now();
        sessions.retain(|_, session| now < session.remove_at);
        sessions
            .get(key_id)
            .map(|session| {
                let mut view = session.view.clone();
                view.active = now < session.deadline;
                view
            })
            .unwrap_or_default()
    }

    fn set(&self, key_id: &str, enabled: bool) -> anyhow::Result<()> {
        let mut sessions = self.0.lock();
        let now = Instant::now();
        sessions.retain(|_, session| now < session.remove_at);
        if enabled {
            ensure!(
                sessions.contains_key(key_id) || sessions.len() < 16,
                invalid(
                    "SESSION_LIMIT",
                    "At most 16 request recognition sessions can be retained; clear an existing session first"
                )
            );
            sessions.insert(
                key_id.to_owned(),
                Session {
                    deadline: now + Duration::from_secs(CAPTURE_SECONDS),
                    remove_at: now + Duration::from_secs(RESULT_SECONDS),
                    view: RequestModelCapture {
                        active: true,
                        expires_at: Some(
                            (chrono::Utc::now()
                                + chrono::Duration::seconds(CAPTURE_SECONDS as i64))
                            .to_rfc3339(),
                        ),
                        ..Default::default()
                    },
                },
            );
        } else if let Some(session) = sessions.get_mut(key_id) {
            session.deadline = now;
            session.view.active = false;
        }
        Ok(())
    }

    pub(crate) fn clear(&self, key_id: &str) {
        self.0.lock().remove(key_id);
    }

    /// Called only after API Key authentication. No raw body, headers, message
    /// text, tool schema or credentials enter the retained sample.
    pub(crate) fn observe(
        &self,
        key_id: &str,
        protocol: &str,
        envelope: &RawEnvelope,
        request: &AiRequest,
    ) {
        let mut sessions = self.0.lock();
        let now = Instant::now();
        sessions.retain(|_, session| now < session.remove_at);
        let Some(session) = sessions.get_mut(key_id).filter(|s| now < s.deadline) else {
            return;
        };
        if !valid_name(&request.model) {
            return;
        }
        let transport = if envelope
            .headers
            .get("upgrade")
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
        {
            "websocket"
        } else {
            "http"
        };
        let mut sample = RequestModelSample {
            request_model: request.model.clone(),
            protocol: protocol.to_owned(),
            transport: transport.into(),
            stream: request.stream.enabled,
            input_items: request.items.len(),
            tools: request.tools.as_ref().map_or(0, Vec::len)
                + request
                    .items
                    .iter()
                    .filter_map(|item| item.unknown_ref())
                    .filter(|raw| {
                        raw.get("type").and_then(serde_json::Value::as_str)
                            == Some("additional_tools")
                    })
                    .filter_map(|raw| raw.get("tools").and_then(serde_json::Value::as_array))
                    .flatten()
                    .map(|tool| {
                        tool.get("tools")
                            .and_then(serde_json::Value::as_array)
                            .map_or(1, Vec::len)
                    })
                    .sum::<usize>(),
            count: 0,
        };
        if let Some(existing) = session.view.samples.iter_mut().find(|s| {
            s.request_model == sample.request_model
                && s.protocol == sample.protocol
                && s.transport == sample.transport
                && s.stream == sample.stream
                && s.input_items == sample.input_items
                && s.tools == sample.tools
        }) {
            existing.count = existing.count.saturating_add(1);
        } else if session.view.samples.len() < MAX_SAMPLES {
            sample.count = 1;
            session.view.samples.push(sample);
        } else {
            session.view.dropped_samples = session.view.dropped_samples.saturating_add(1);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestModelSuggestion {
    pub request_model: String,
    pub target_model_id: Option<String>,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestModelsView {
    pub config: RequestModelConfig,
    pub eligible_models: Vec<RequestModelTarget>,
    pub capture: RequestModelCapture,
    pub suggestions: Vec<RequestModelSuggestion>,
}

pub(crate) fn suggestions(
    samples: &[RequestModelSample],
    routes: &[RouteConfig],
) -> Vec<RequestModelSuggestion> {
    let names = samples
        .iter()
        .map(|s| s.request_model.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    names
        .into_iter()
        .map(|name| {
            // Exact public name, account prefix suffix, or exact configured upstream
            // name only. Never guess another model family/version by fuzzy similarity.
            let candidates = routes
                .iter()
                .filter(|route| {
                    route.model_id.as_str() == name
                        || route.model_id.rsplit('/').next() == Some(name)
                        || route
                            .targets
                            .iter()
                            .any(|target| target.model().as_str() == name)
                })
                .map(|route| route.id.to_string())
                .collect::<Vec<_>>();
            RequestModelSuggestion {
                request_model: name.into(),
                target_model_id: (candidates.len() == 1).then(|| candidates[0].clone()),
                candidates,
            }
        })
        .collect()
}

pub(crate) async fn view(gateway: &Gateway, key_id: &str) -> anyhow::Result<RequestModelsView> {
    let key = gateway.admin().get_api_key(key_id).await?;
    let config = load(gateway, key_id).await?;
    let routes = gateway
        .model_cache
        .read()
        .await
        .models
        .iter()
        .filter(|route| {
            (key.model_ids.is_empty() && key.is_enabled)
                || key.model_ids.iter().any(|id| id == route.id.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    let capture = gateway.request_model_capture.view(key_id);
    Ok(RequestModelsView {
        config,
        suggestions: suggestions(&capture.samples, &routes),
        capture,
        eligible_models: routes
            .iter()
            .map(|route| RequestModelTarget {
                id: route.id.to_string(),
                model_id: route.model_id.to_string(),
                display_name: route.effective_display_name().into(),
            })
            .collect(),
    })
}

pub(crate) async fn save(
    gateway: &Gateway,
    key_id: &str,
    config: RequestModelConfig,
) -> anyhow::Result<()> {
    config.validate()?;
    let key = gateway.admin().get_api_key(key_id).await?;
    let routes = gateway.model_cache.read().await;
    for mapping in config.mappings.iter().filter(|_| config.enabled) {
        let target = routes
            .models
            .iter()
            .find(|route| route.id.as_str() == mapping.target_model_id)
            .ok_or_else(|| invalid("TARGET_MISSING", "The mapped target model no longer exists"))?;
        ensure!(
            (key.model_ids.is_empty() && key.is_enabled)
                || key.model_ids.iter().any(|id| id == target.id.as_str()),
            invalid(
                "TARGET_DENIED",
                "The mapped target model is not authorized for this API Key"
            )
        );
        ensure!(
            !routes
                .models
                .iter()
                .any(|route| route.id.as_str() == mapping.request_model),
            invalid(
                "INTERNAL_KEY",
                "A request model name cannot override an internal Route key"
            )
        );
    }
    drop(routes);
    gateway
        .storage
        .settings()
        .set(
            &format!("{SETTING_PREFIX}{key_id}"),
            &serde_json::to_string(&config)?,
        )
        .await?;
    crate::storage::bump_config_epoch(gateway.storage.settings()).await?;
    Ok(())
}

pub(crate) fn set_capture(gateway: &Gateway, key_id: &str, enabled: bool) -> anyhow::Result<()> {
    gateway.request_model_capture.set(key_id, enabled)
}

#[cfg(test)]
mod tests;
