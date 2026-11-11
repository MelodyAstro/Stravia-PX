use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use stravia_platform_plugin_runtime::LoadedPlugin;
use stravia_runtime_contract::CancellationToken;
use stravia_runtime_contract::hook::*;
use stravia_runtime_contract::protocol::ir::{AiItem, AiRequest, AiResponse};

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
        .filter(|snapshot| {
            !snapshot.loaded.manifest().tools.is_empty()
                || snapshot
                    .loaded
                    .manifest()
                    .hooks
                    .iter()
                    .any(|kind| *kind != HookKind::ResolveModel)
        })
        .map(|snapshot| Arc::new(PluginHook(snapshot.clone())) as Arc<dyn Hook>)
        .collect()
}

pub(crate) fn mcp_tools(
    snapshots: &[PluginSnapshot],
    storage: &crate::storage::DynStorage,
) -> Vec<Arc<dyn crate::mcp::McpTool>> {
    snapshots
        .iter()
        .flat_map(|snapshot| {
            snapshot
                .loaded
                .manifest()
                .tools
                .iter()
                .map(move |manifest| {
                    Arc::new(PluginMcpTool {
                        tool: PluginTool::new(snapshot.clone(), manifest.clone()),
                        storage: Arc::clone(storage),
                    }) as Arc<dyn crate::mcp::McpTool>
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
        .take(24)
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

struct PluginMcpTool {
    tool: PluginTool,
    storage: crate::storage::DynStorage,
}

impl PluginMcpTool {
    async fn authorized(
        &self,
        context: &crate::mcp::McpContext,
    ) -> Result<bool, crate::mcp::McpToolError> {
        let Some(keys) = self.storage.api_keys() else {
            return Ok(false);
        };
        let key = keys.get(&context.api_key_id).await.map_err(|error| {
            crate::mcp::McpToolError::new("mcp_access_check_failed", error.to_string())
        })?;
        if !key.is_some_and(|key| key.is_enabled && key.mcp_access_enabled) {
            return Ok(false);
        }
        Ok(crate::proxy::security::Security::new(self.storage.auth())
            .authorize_principal_capability(&stravia_runtime_contract::Principal::new(
                context.api_key_id.clone(),
            ))
            .await
            .is_ok())
    }
}

#[async_trait]
impl crate::mcp::McpTool for PluginMcpTool {
    fn name(&self) -> &str {
        &self.tool.name
    }
    fn description(&self) -> Option<&str> {
        Some(&self.tool.manifest.description)
    }
    fn input_schema(&self) -> Value {
        self.tool.manifest.parameters.clone()
    }
    async fn available(
        &self,
        context: &crate::mcp::McpContext,
    ) -> Result<bool, crate::mcp::McpToolError> {
        self.authorized(context).await
    }
    async fn call(
        &self,
        arguments: Value,
        context: &crate::mcp::McpContext,
    ) -> Result<crate::mcp::McpToolOutput, crate::mcp::McpToolError> {
        // Component/configuration snapshots stay pinned for an admitted call;
        // permission changes must still apply to a retained tool handle.
        if !self.authorized(context).await? {
            return Err(crate::mcp::McpToolError::new(
                "tool_unavailable",
                "Plugin tool is not available for this API key",
            ));
        }
        let cancellation = context
            .execution()
            .map(|(cancellation, _)| cancellation)
            .unwrap_or_default();
        let output = self
            .tool
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
            .filter_map(|kind| event_kind(*kind))
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
        let request_scope = match &event {
            HookEvent::Request {
                current,
                input_start,
                ..
            } => Some((*current, *input_start)),
            _ => None,
        };
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
                actions.extend(action.into_actions(request_scope)?);
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

fn event_kind(kind: HookKind) -> Option<EventKind> {
    Some(match kind {
        HookKind::ResolveModel => return None,
        HookKind::Request => EventKind::Request,
        HookKind::UpstreamResponse => EventKind::UpstreamResponse,
        HookKind::ToolResult => EventKind::ToolResult,
        HookKind::ClientOutput => EventKind::ClientOutput,
    })
}

fn event_input(event: HookEvent<'_>) -> anyhow::Result<(HookKind, &'static str, Value)> {
    let (kind, operation, mut input, route) = match event {
        HookEvent::Request {
            current,
            input_start,
            round,
            route,
            ..
        } => (
            HookKind::Request,
            "request",
            json!({"request": current, "input_start": input_start, "round": round}),
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
    for pointer in ["/request/meta/vendor/egress", "/response/vendor/egress"] {
        if let Some(private_annotations) = input.pointer_mut(pointer) {
            *private_annotations = json!({});
        }
    }
    for pointer in ["/request/meta/vendor", "/response/vendor"] {
        if let Some(vendor_annotations) = input.pointer_mut(pointer) {
            *vendor_annotations = crate::hook::redact_vendor_value(vendor_annotations);
        }
    }
    // Plugin payloads contain canonical business data only. Provider snapshots,
    // connection state, auth headers and the API key principal never cross here.
    Ok((kind, operation, input))
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
    SpliceInput {
        start: usize,
        end: usize,
        items: Vec<AiItem>,
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
    fn into_actions(
        self,
        request_scope: Option<(&AiRequest, usize)>,
    ) -> Result<Vec<HookAction>, String> {
        if let Self::ReplaceRequest { value } = self {
            let (current, input_start) =
                request_scope.ok_or("replace_request requires a request event")?;
            return legacy_request_actions(current, input_start, *value);
        }
        Ok(vec![match self {
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
            Self::SpliceInput { start, end, items } => {
                HookAction::PatchRequest(Box::new(RequestPatch::SpliceInput {
                    range: start..end,
                    items,
                }))
            }
            Self::ReplaceRequest { .. } => unreachable!("handled above"),
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
        }])
    }
}

/// Translate the v1 whole-request action into admitted v0.6.5 patches. Already
/// processed history and host metadata cannot be rewritten by this adapter.
fn legacy_request_actions(
    current: &AiRequest,
    input_start: usize,
    next: AiRequest,
) -> Result<Vec<HookAction>, String> {
    if input_start > current.items.len() || input_start > next.items.len() {
        return Err("replace_request removed processed history".into());
    }
    let mut before = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let mut after = serde_json::to_value(&next).map_err(|e| e.to_string())?;
    for key in [
        "model",
        "instructions",
        "items",
        "generation",
        "tools",
        "tool_choice",
        "parameters",
    ] {
        before.as_object_mut().unwrap().remove(key);
        after.as_object_mut().unwrap().remove(key);
    }
    if before != after
        || serde_json::to_value(&current.items[..input_start]).map_err(|e| e.to_string())?
            != serde_json::to_value(&next.items[..input_start]).map_err(|e| e.to_string())?
    {
        return Err("replace_request changed protected fields or processed history; use splice_input for new input".into());
    }
    let patches = vec![
        RequestPatch::SetModel(next.model),
        RequestPatch::SetSystem(next.instructions),
        RequestPatch::SetGeneration(next.generation),
        RequestPatch::ReplaceTools(next.tools),
        RequestPatch::SetToolChoice(next.tool_choice),
        RequestPatch::SetParameters(Box::new(next.parameters)),
        RequestPatch::SpliceInput {
            range: 0..current.items.len() - input_start,
            items: next.items.into_iter().skip(input_start).collect(),
        },
    ];
    Ok(patches
        .into_iter()
        .map(|patch| HookAction::PatchRequest(Box::new(patch)))
        .collect())
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
        let tool_schema = json!({
            "type":"object",
            "properties":{"token":{"type":"string"},"api_key":{"type":"string"}},
            "required":["token","api_key"],
            "additionalProperties":false
        });
        request.tools = Some(vec![stravia_runtime_contract::protocol::ir::ToolSpec {
            name: "business_tool".into(),
            description: None,
            parameters: tool_schema.clone(),
            strict: Some(true),
            defer_loading: None,
        }]);
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
            input_start: 0,
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
        assert_eq!(value["request"]["tools"][0]["parameters"], tool_schema);
    }

    #[test]
    fn tool_result_business_keys_are_preserved_in_hook_payloads() {
        let context = SessionContext {
            tools_fixed: false,
            request_id: "request".into(),
            run_id: "run".into(),
            request_kind: RequestKind::Generation,
            ingress: stravia_runtime_contract::protocol::ids::OPENAI_COMPATIBLE_CHAT_COMPLETIONS_V1,
            transport: TransportKind::Http,
            principal: stravia_runtime_contract::Principal::new("test-key"),
            cancellation: Default::default(),
            inherited_media_turns: Vec::new(),
            response_id: None,
            previous_response_id: None,
        };
        let content = json!({"token":"pagination-token", "secret":false, "nested":{"api_key":"business-field"}});
        let metadata = json!({"secret":"business-metadata"})
            .as_object()
            .unwrap()
            .clone();
        let result = PlatformToolResult {
            tool_id: ToolId::new("business-tool"),
            call_id: "call".into(),
            content: content.clone(),
            content_kind: stravia_runtime_contract::protocol::ir::ToolResultContentKind::Json,
            is_error: false,
            metadata: metadata.clone(),
        };
        let route = RouteContext {
            model_id: "model".into(),
            provider_id: "private-provider".into(),
            target_id: "target".into(),
            egress: None,
        };
        let (_, _, payload) = event_input(HookEvent::ToolResult {
            session: &context,
            result: &result,
            route: &route,
            round: 0,
        })
        .unwrap();
        assert_eq!(payload["result"]["content"], content);
        assert_eq!(payload["result"]["metadata"], json!(metadata));
        assert!(!payload.to_string().contains("private-provider"));
    }

    #[test]
    fn hook_rejections_and_unknown_actions_are_validated() {
        let action: PluginAction = serde_json::from_value(
            json!({"type":"reject","status":200,"code":"ok","message":"invalid"}),
        )
        .unwrap();
        assert!(action.into_actions(None).is_err());
        assert!(
            serde_json::from_value::<PluginActions>(
                json!({"actions":[{"type":"execute_shell","value":"command"}]})
            )
            .is_err()
        );
    }

    #[test]
    fn every_hook_payload_excludes_private_egress_annotations() {
        let context = SessionContext {
            tools_fixed: false,
            request_id: "request".into(),
            run_id: "run".into(),
            request_kind: RequestKind::Generation,
            ingress: stravia_runtime_contract::protocol::ids::OPENAI_COMPATIBLE_CHAT_COMPLETIONS_V1,
            transport: TransportKind::Http,
            principal: stravia_runtime_contract::Principal::new("test-key"),
            cancellation: Default::default(),
            inherited_media_turns: Vec::new(),
            response_id: None,
            previous_response_id: None,
        };
        let mut request = AiRequest::new("model", Vec::new());
        request
            .meta
            .vendor
            .egress
            .insert("opaque".into(), json!("private-request-state"));
        let snapshot = ContextSnapshot::from_request(&request, ContextCompleteness::Full);
        let mut response = AiResponse::new("response", "model");
        response
            .vendor
            .egress
            .insert("opaque".into(), json!("private-response-state"));
        response
            .vendor
            .ingress
            .insert("business_value".into(), json!(42));
        let route = RouteContext {
            model_id: "model".into(),
            provider_id: "provider".into(),
            target_id: "target".into(),
            egress: None,
        };
        let classified = ClassifiedToolCalls::default();
        let events = [
            HookEvent::Request {
                session: &context,
                original: &snapshot,
                read_scope: ReadExposureScope::NONE,
                current: &request,
                input_start: 0,
                context: &snapshot,
                route: Some(&route),
                round: 0,
            },
            HookEvent::UpstreamResponse {
                session: &context,
                request: &request,
                response: &response,
                classified: &classified,
                route: &route,
                round: 0,
            },
            HookEvent::ClientOutput {
                session: &context,
                response: &response,
                route: &route,
                round: 0,
            },
        ];
        for event in events {
            let (_, _, payload) = event_input(event).unwrap();
            let encoded = payload.to_string();
            assert!(!encoded.contains("private-request-state"));
            assert!(!encoded.contains("private-response-state"));
            if payload.get("request").is_some() {
                assert_eq!(payload["request"]["meta"]["vendor"]["egress"], json!({}));
            }
            if payload.get("response").is_some() {
                assert_eq!(payload["response"]["vendor"]["egress"], json!({}));
                assert_eq!(
                    payload["response"]["vendor"]["ingress"]["business_value"],
                    42
                );
            }
        }
        assert!(
            !request.meta.vendor.egress.is_empty(),
            "sanitizing a view must not mutate the request"
        );
        assert!(
            !response.vendor.egress.is_empty(),
            "sanitizing a view must not mutate the response"
        );
    }

    struct NamedTool(String);

    #[async_trait]
    impl PlatformTool for NamedTool {
        fn id(&self) -> ToolId {
            ToolId::new("maximum-name")
        }
        fn external_name(&self) -> &str {
            &self.0
        }
        fn parameters(&self) -> Value {
            json!({"type":"object"})
        }
        async fn execute(
            &self,
            arguments: Value,
            _context: ToolExecutionContext,
        ) -> Result<Value, PlatformToolError> {
            Ok(arguments)
        }
    }

    #[test]
    fn maximum_tool_names_keep_the_complete_identity_across_transports() {
        let raw_name = "x".repeat(64);
        let name = external_name("example.plugin", &raw_name);
        assert_eq!(name.len(), 48);
        let registry =
            crate::hook::PlatformToolRegistry::new(vec![Arc::new(NamedTool(name.clone()))])
                .unwrap();
        let exposed = registry
            .expose(&ToolId::new("maximum-name"), &Default::default())
            .unwrap();
        assert_eq!(exposed.provider_name, format!("stravia__{name}"));
        assert_ne!(
            name,
            external_name("example.plugin", &format!("{}y", "x".repeat(63))),
            "the hash must include the complete untruncated identity"
        );
    }
}
