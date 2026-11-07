use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use stravia_platform_plugin_runtime::LoadedPlugin;
use stravia_runtime_contract::CancellationToken;
use stravia_runtime_contract::hook::*;
use stravia_runtime_contract::protocol::ir::{AiRequest, AiResponse};

use super::{HookKind, ToolManifest, schema};

#[derive(Clone)]
pub(crate) struct PluginSnapshot {
    pub(super) loaded: LoadedPlugin,
    pub(super) config: Value,
}

pub(crate) fn snapshot_tools(snapshots: &[PluginSnapshot]) -> Vec<Arc<dyn PlatformTool>> {
    snapshots
        .iter()
        .flat_map(|snapshot| {
            snapshot
                .loaded
                .manifest()
                .tools
                .iter()
                .map(move |manifest| {
                    Arc::new(PluginTool::new(snapshot.clone(), manifest.clone()))
                        as Arc<dyn PlatformTool>
                })
        })
        .collect()
}

pub(crate) fn snapshot_hooks(snapshots: &[PluginSnapshot]) -> Vec<Arc<dyn Hook>> {
    snapshots
        .iter()
        .map(|snapshot| Arc::new(PluginHook(snapshot.clone())) as Arc<dyn Hook>)
        .collect()
}

pub(crate) fn mcp_tools(snapshots: &[PluginSnapshot]) -> Vec<Arc<dyn crate::mcp::McpTool>> {
    snapshots
        .iter()
        .flat_map(|snapshot| {
            snapshot
                .loaded
                .manifest()
                .tools
                .iter()
                .map(move |manifest| {
                    Arc::new(PluginTool::new(snapshot.clone(), manifest.clone()))
                        as Arc<dyn crate::mcp::McpTool>
                })
        })
        .collect()
}

fn tool_id(plugin_id: &str, name: &str) -> ToolId {
    ToolId::new(format!("platform-plugin:{plugin_id}:{name}"))
}

fn external_name(plugin_id: &str, name: &str) -> String {
    // The full identity participates in the hash; truncation cannot alias tools.
    let digest = Sha256::digest(format!("{plugin_id}:{name}").as_bytes());
    let hash = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let readable: String = name
        .chars()
        .take(28)
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("plugin_{readable}_{hash}")
}

struct PluginTool {
    snapshot: PluginSnapshot,
    manifest: ToolManifest,
    name: String,
}

impl PluginTool {
    fn new(snapshot: PluginSnapshot, manifest: ToolManifest) -> Self {
        let name = external_name(&snapshot.loaded.manifest().id, &manifest.name);
        Self {
            snapshot,
            manifest,
            name,
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        cancellation: CancellationToken,
    ) -> anyhow::Result<Value> {
        schema::validate(&self.manifest.parameters, &arguments)?;
        let input = json!({"config": self.snapshot.config, "arguments": arguments});
        let operation = format!("tool:{}", self.manifest.name);
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => anyhow::bail!("plugin tool execution cancelled"),
            result = self.snapshot.loaded.invoke(&operation, &input) => result,
        }
    }
}

#[async_trait]
impl PlatformTool for PluginTool {
    fn id(&self) -> ToolId {
        tool_id(&self.snapshot.loaded.manifest().id, &self.manifest.name)
    }
    fn external_name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> Option<&str> {
        Some(&self.manifest.description)
    }
    fn parameters(&self) -> Value {
        self.manifest.parameters.clone()
    }
    fn parallel_safe(&self) -> bool {
        self.manifest.parallel_safe
    }
    fn strict_schema(&self) -> bool {
        false
    }
    async fn execute(
        &self,
        arguments: Value,
        context: ToolExecutionContext,
    ) -> Result<Value, PlatformToolError> {
        self.invoke(arguments, context.cancellation)
            .await
            .map_err(|error| PlatformToolError::new(error.to_string()))
    }
}

#[async_trait]
impl crate::mcp::McpTool for PluginTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> Option<&str> {
        Some(&self.manifest.description)
    }
    fn input_schema(&self) -> Value {
        self.manifest.parameters.clone()
    }
    async fn available(
        &self,
        _context: &crate::mcp::McpContext,
    ) -> Result<bool, crate::mcp::McpToolError> {
        Ok(true)
    }
    async fn call(
        &self,
        arguments: Value,
        context: &crate::mcp::McpContext,
    ) -> Result<crate::mcp::McpToolOutput, crate::mcp::McpToolError> {
        let cancellation = context
            .execution()
            .map(|(cancellation, _)| cancellation)
            .unwrap_or_default();
        let output = self
            .invoke(arguments, cancellation)
            .await
            .map_err(|error| crate::mcp::McpToolError::new("plugin_failed", error.to_string()))?;
        Ok(crate::mcp::McpToolOutput::success(
            json!({"result": output}),
        ))
    }
}

struct PluginHook(PluginSnapshot);

impl Hook for PluginHook {
    fn descriptor(&self) -> HookDescriptor {
        let mut descriptor =
            HookDescriptor::all(format!("platform-plugin:{}", self.0.loaded.manifest().id));
        descriptor.event_kinds = self
            .0
            .loaded
            .manifest()
            .hooks
            .iter()
            .map(|kind| event_kind(*kind))
            .collect();
        if !self.0.loaded.manifest().tools.is_empty()
            && !descriptor.event_kinds.contains(&EventKind::Request)
        {
            descriptor.event_kinds.push(EventKind::Request);
        }
        descriptor
    }
    fn create_session(&self, context: &SessionContext) -> Box<dyn HookSession> {
        Box::new(PluginHookSession {
            snapshot: self.0.clone(),
            cancellation: context.cancellation.clone(),
            tools_fixed: context.tools_fixed || context.request_kind != RequestKind::Generation,
        })
    }
}

struct PluginHookSession {
    snapshot: PluginSnapshot,
    cancellation: CancellationToken,
    tools_fixed: bool,
}

#[async_trait]
impl HookSession for PluginHookSession {
    async fn handle(&mut self, event: HookEvent<'_>) -> Result<ActionBatch, String> {
        let kind = match event.kind() {
            EventKind::Request => HookKind::Request,
            EventKind::UpstreamResponse => HookKind::UpstreamResponse,
            EventKind::ToolResult => HookKind::ToolResult,
            EventKind::ClientOutput => HookKind::ClientOutput,
            EventKind::Stream => return Ok(ActionBatch::default()),
        };
        let mut actions = Vec::new();
        if self.snapshot.loaded.manifest().hooks.contains(&kind) {
            let (_, operation, mut input) =
                event_input(event).map_err(|error| error.to_string())?;
            input["config"] = self.snapshot.config.clone();
            let output = tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => return Err("plugin hook execution cancelled".into()),
                result = self.snapshot.loaded.invoke(operation, &input) => result.map_err(|error| error.to_string())?,
            };
            let batch: PluginActions = serde_json::from_value(output)
                .map_err(|error| format!("invalid plugin hook actions: {error}"))?;
            if batch.actions.len() > 64 {
                return Err("plugin hook returned more than 64 actions".into());
            }
            for action in batch.actions {
                actions.push(action.into_action()?);
            }
        }
        if matches!(kind, HookKind::Request) && !self.tools_fixed {
            actions.extend(self.snapshot.loaded.manifest().tools.iter().map(|tool| {
                HookAction::ExposeTool(tool_id(&self.snapshot.loaded.manifest().id, &tool.name))
            }));
        }
        Ok(ActionBatch { actions })
    }
    // Terminal hooks require the existing buffer path so clients never receive
    // the unmodified response before the plugin's result is admitted.
    fn requires_terminal_buffering(&self) -> bool {
        self.snapshot
            .loaded
            .manifest()
            .hooks
            .iter()
            .any(|kind| matches!(kind, HookKind::UpstreamResponse | HookKind::ClientOutput))
    }
}

fn event_kind(kind: HookKind) -> EventKind {
    match kind {
        HookKind::Request => EventKind::Request,
        HookKind::UpstreamResponse => EventKind::UpstreamResponse,
        HookKind::ToolResult => EventKind::ToolResult,
        HookKind::ClientOutput => EventKind::ClientOutput,
    }
}

fn event_input(event: HookEvent<'_>) -> anyhow::Result<(HookKind, &'static str, Value)> {
    let (kind, operation, mut input, route) = match event {
        HookEvent::Request {
            current,
            round,
            route,
            ..
        } => (
            HookKind::Request,
            "request",
            json!({"request": current, "round": round}),
            route,
        ),
        HookEvent::UpstreamResponse {
            request,
            response,
            round,
            route,
            ..
        } => (
            HookKind::UpstreamResponse,
            "upstream_response",
            json!({"request": request, "response": response, "round": round}),
            Some(route),
        ),
        HookEvent::ToolResult {
            result,
            round,
            route,
            ..
        } => (
            HookKind::ToolResult,
            "tool_result",
            json!({"result": {"tool_id":result.tool_id.as_str(), "call_id":result.call_id, "content":result.content, "is_error":result.is_error, "metadata":result.metadata}, "round":round}),
            Some(route),
        ),
        HookEvent::ClientOutput {
            response,
            round,
            route,
            ..
        } => (
            HookKind::ClientOutput,
            "client_output",
            json!({"response":response, "round":round}),
            Some(route),
        ),
    };
    if let Some(route) = route {
        input["route"] = json!({"model_id":route.model_id, "target_id":route.target_id, "egress":route.egress.as_ref().map(ToString::to_string)});
    }
    input["event"] = Value::String(operation.to_owned());
    // Plugin payloads contain canonical business data only. Provider snapshots,
    // connection state, auth headers and the API key principal never cross here.
    Ok((kind, operation, crate::hook::redact_vendor_value(&input)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginActions {
    actions: Vec<PluginAction>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum PluginAction {
    SetSystem {
        value: Option<String>,
    },
    SetModel {
        value: String,
    },
    SetContent {
        value: String,
    },
    SetReasoning {
        value: Option<String>,
    },
    ReplaceRequest {
        value: Box<AiRequest>,
    },
    ReplaceResponse {
        value: Box<AiResponse>,
    },
    SetToolResult {
        value: Value,
    },
    Reject {
        status: u16,
        code: String,
        message: String,
    },
}

impl PluginAction {
    fn into_action(self) -> Result<HookAction, String> {
        Ok(match self {
            Self::SetSystem { value } => {
                HookAction::PatchRequest(Box::new(RequestPatch::SetSystem(value)))
            }
            Self::SetModel { value } => {
                HookAction::PatchRequest(Box::new(RequestPatch::SetModel(value)))
            }
            Self::SetContent { value } => {
                HookAction::PatchResponse(ResponsePatch::SetContent(value))
            }
            Self::SetReasoning { value } => {
                HookAction::PatchResponse(ResponsePatch::SetReasoning(value))
            }
            Self::ReplaceRequest { value } => {
                HookAction::PatchRequest(Box::new(RequestPatch::ReplaceCanonical(value)))
            }
            Self::ReplaceResponse { value } => {
                HookAction::PatchResponse(ResponsePatch::ReplaceCanonical(value))
            }
            Self::SetToolResult { value } => {
                HookAction::PatchToolResult(ToolResultPatch::SetContent(value))
            }
            Self::Reject {
                status,
                code,
                message,
            } => {
                if !(400..=599).contains(&status) || code.trim().is_empty() {
                    return Err(
                        "plugin rejection needs an HTTP error status and nonempty code".into(),
                    );
                }
                HookAction::Reject(HookRejection {
                    status,
                    code,
                    message,
                })
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_payload_omits_credentials_and_private_provider_identity() {
        let context = SessionContext {
            tools_fixed: false,
            request_id: "private-request".into(),
            run_id: "private-run".into(),
            request_kind: RequestKind::Generation,
            ingress: stravia_runtime_contract::protocol::ids::OPENAI_COMPATIBLE_CHAT_COMPLETIONS_V1,
            transport: TransportKind::Http,
            principal: stravia_runtime_contract::Principal::new("private-key"),
            cancellation: Default::default(),
            inherited_media_turns: Vec::new(),
            response_id: None,
            previous_response_id: None,
        };
        let mut request = AiRequest::new("model", Vec::new());
        request.meta.vendor.ingress.insert(
            "nested".into(),
            json!({"Authorization":"private-auth", "api_key":"private-api", "value":42}),
        );
        let snapshot = ContextSnapshot::from_request(&request, ContextCompleteness::Full);
        let route = RouteContext {
            model_id: "model".into(),
            provider_id: "private-provider".into(),
            target_id: "target".into(),
            egress: None,
        };
        let (_, _, value) = event_input(HookEvent::Request {
            session: &context,
            original: &snapshot,
            read_scope: ReadExposureScope::NONE,
            current: &request,
            context: &snapshot,
            route: Some(&route),
            round: 0,
        })
        .unwrap();
        let encoded = value.to_string();
        for secret in [
            "private-key",
            "private-provider",
            "private-auth",
            "private-api",
            "private-request",
            "private-run",
        ] {
            assert!(!encoded.contains(secret));
        }
        assert_eq!(
            value["request"]["meta"]["vendor"]["ingress"]["nested"]["value"],
            42
        );
    }

    #[test]
    fn hook_rejections_and_unknown_actions_are_validated() {
        let action: PluginAction = serde_json::from_value(
            json!({"type":"reject","status":200,"code":"ok","message":"invalid"}),
        )
        .unwrap();
        assert!(action.into_action().is_err());
        assert!(
            serde_json::from_value::<PluginActions>(
                json!({"actions":[{"type":"execute_shell","value":"command"}]})
            )
            .is_err()
        );
    }
}
