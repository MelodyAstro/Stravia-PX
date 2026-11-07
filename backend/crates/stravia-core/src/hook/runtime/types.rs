use super::*;

#[derive(Clone, Default)]
pub struct HookRuntime {
    pub(super) hooks: Arc<[Arc<dyn Hook>]>,
    pub(super) tools: PlatformToolRegistry,
    pub(super) platform_plugins: Option<Arc<crate::platform_plugins::PlatformPlugins>>,
    pub(super) plugin_snapshot: Option<Arc<[crate::platform_plugins::PluginSnapshot]>>,
}
