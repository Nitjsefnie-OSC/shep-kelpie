//! The harness MCP port: which MCP servers a harness cannot load, and what it drops of those it can

use std::fmt;

use serde_json::Value;

use crate::settings::AgentHarness;

/// What each harness says of a Claude-shaped `mcpServers` entry
pub trait HarnessMcp: Send + Sync + fmt::Debug {
    /// Why `harness` cannot load the server `name`, if it cannot
    ///
    /// A harness that loads every server answers `None`.
    fn refusal(&self, harness: &AgentHarness, name: &str, entry: &Value) -> Option<String>;

    /// What `harness` drops of the server `name`, which it loads without, if it drops anything
    fn skipped_env(&self, harness: &AgentHarness, name: &str, entry: &Value) -> Option<String>;
}
