//! Installable platform extensions. Each request pins immutable component and
//! configuration snapshots; publishing an update only affects later requests.
mod adapters;
mod manager;
mod schema;

pub(crate) use adapters::{PluginSnapshot, mcp_tools, snapshot_hooks, snapshot_tools};
pub(crate) use manager::PlatformPlugins;
pub use manager::{PlatformPluginSummary, PlatformPluginUpdate};
pub use stravia_platform_plugin_sdk::{HookKind, PluginManifest, ToolManifest};

#[cfg(test)]
mod tests;
