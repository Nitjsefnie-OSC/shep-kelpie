//! `[app.dogs.kelpie.code_graph]`: whether reviewers and, with `worker`, workers get a code graph

use schemars::JsonSchema;
use serde::Deserialize;

/// Whether the project's workers and reviewers are given a code graph
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Code graph")]
pub struct CodeGraph {
    /// Whether kelpie indexes each work item's worktree and gives the reviewers'
    /// and the triager's calls the codebase-memory-mcp server over it, and the
    /// worker's own working turns too with `worker`. `shep kelpie tools install`
    /// fetches the server. Off when absent.
    #[serde(default)]
    pub enabled: bool,

    /// Whether the worker's own working turns get the server too, off when
    /// absent. It adds the server's 17 tool definitions to every request of
    /// every worker turn, and no worker session recorded so far, on Claude Code
    /// or pi, has answered one of them (`docs/facts/mcp-servers.md`). Name the
    /// key when your workers really read the graph.
    #[serde(default)]
    pub worker: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_graph_is_off_unless_the_project_turns_it_on() {
        assert!(!CodeGraph::default().enabled);
        assert!(!CodeGraph::default().worker);
        let on: CodeGraph = serde_json::from_value(serde_json::json!({ "enabled": true })).unwrap();
        assert!(on.enabled);
        assert!(!on.worker, "a worker is named by a key of its own");
        let none: CodeGraph = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(!none.enabled);
        assert!(!none.worker);
    }

    #[test]
    fn a_project_can_name_the_worker_too() {
        let named: CodeGraph =
            serde_json::from_value(serde_json::json!({ "enabled": true, "worker": true })).unwrap();
        assert!(named.enabled);
        assert!(named.worker);
        let off: CodeGraph =
            serde_json::from_value(serde_json::json!({ "enabled": true, "worker": false }))
                .unwrap();
        assert!(off.enabled);
        assert!(!off.worker);
    }

    #[test]
    fn a_key_the_table_does_not_have_is_refused() {
        let wrong = serde_json::json!({ "enable": true });
        assert!(serde_json::from_value::<CodeGraph>(wrong).is_err());
        let wrong = serde_json::json!({ "enabled": true, "woker": true });
        assert!(serde_json::from_value::<CodeGraph>(wrong).is_err());
    }
}
