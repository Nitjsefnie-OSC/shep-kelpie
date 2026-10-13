//! `[app.dogs.kelpie.code_graph]`: whether workers and reviewers get a code graph of the worktree

use schemars::JsonSchema;
use serde::Deserialize;

/// Whether the project's workers and reviewers are given a code graph
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Code graph")]
pub struct CodeGraph {
    /// Whether kelpie indexes each work item's worktree before its first turn
    /// and gives the worker's and the reviewers' Claude Code calls the
    /// codebase-memory-mcp server over it. `shep kelpie tools install` fetches
    /// the server. Off when absent.
    #[serde(default)]
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_graph_is_off_unless_the_project_turns_it_on() {
        assert!(!CodeGraph::default().enabled);
        let on: CodeGraph = serde_json::from_value(serde_json::json!({ "enabled": true })).unwrap();
        assert!(on.enabled);
        let none: CodeGraph = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(!none.enabled);
    }

    #[test]
    fn a_key_the_table_does_not_have_is_refused() {
        let wrong = serde_json::json!({ "enable": true });
        assert!(serde_json::from_value::<CodeGraph>(wrong).is_err());
    }
}
