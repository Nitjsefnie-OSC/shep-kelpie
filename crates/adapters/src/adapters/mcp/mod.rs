//! What the sandbox lets the MCP servers a call loads reach, whichever harness runs it
//!
//! The servers themselves are built in [`crate::ports::mcp`].

use crate::ports::{AgentCall, Policy};

/// Lets the sandbox connect to the browser's socket, where the call has one
pub(crate) fn fence_the_browser(policy: &mut Policy, call: &AgentCall) {
    if let Some(browser) = &call.reach.browser {
        policy.sockets.push(browser.socket.clone());
    }
}

/// What the code graph server makes in `/tmp` with a random name: `search_code`'s scratch, the
/// index watcher's git output and an artifact export. It names `/tmp` itself, whatever `TMPDIR`
/// says. Not `cbm-daemon-*`, the default runtime folders of daemons outside kelpie.
const SCRATCH: [&str; 3] = [
    "/private/tmp/cbm-search-*",
    "/private/tmp/cbm-watcher-git-*",
    "/private/tmp/cbm-artifact-*",
];

/// Lets the sandbox read the code graph's binary and index, write its index, bind and reach its
/// daemon's socket in its runtime folder, open each folder above the index and the runtime folder,
/// which the server checks before it starts, and make its scratch in `/tmp`, where the call has
/// one
pub(crate) fn fence_the_graph(policy: &mut Policy, call: &AgentCall) {
    if let Some(graph) = &call.reach.code_graph {
        let folders = [graph.index.clone(), graph.runtime.clone()];
        for above in folders.iter().flat_map(|folder| folder.ancestors().skip(1)) {
            if above.parent().is_some() && !policy.open.iter().any(|open| open == above) {
                policy.open.push(above.to_owned());
            }
        }
        policy.read.push(graph.binary.clone());
        policy.read.extend(folders.clone());
        policy.write.extend(folders);
        policy.sockets.push(graph.runtime.clone());
        policy.scratch.extend(SCRATCH.map(str::to_owned));
    }
}

// The server binds a Unix socket, and the socket check names a uid.
#[cfg(all(test, unix))]
mod live;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::ports::{CodeGraph, Reach, Role, Session, SessionId, Tools};
    use crate::settings::{AgentHarness, Effort};

    fn graph() -> CodeGraph {
        CodeGraph {
            binary: "/k/tools/codebase-memory-mcp".into(),
            index: "/k/graphs/7".into(),
            runtime: "/k/cbm/7".into(),
        }
    }

    fn call(reach: Reach) -> AgentCall {
        AgentCall {
            issue: crate::ticket::TicketId::from_issue(7),
            reach,
            ..AgentCall::new(
                Role::Worker,
                Tools::Work,
                AgentHarness::ClaudeCode,
                "claude-sonnet-5",
                Effort::Medium,
                Session::New(SessionId("7e812e8a-3bf1-42a7-bddf-0ab283b7372a".into())),
                PathBuf::from("/work/7"),
                "go",
            )
        }
    }

    #[test]
    fn the_fence_opens_each_folder_above_the_graphs_once_and_its_scratch_in_tmp() {
        let mut policy = Policy::default();
        fence_the_graph(&mut policy, &call(Reach::default()));
        assert_eq!(policy, Policy::default(), "no graph, no grants");

        let reach = Reach {
            code_graph: Some(graph()),
            ..Reach::default()
        };
        fence_the_graph(&mut policy, &call(reach));
        let open: Vec<&str> = policy.open.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(open, ["/k/graphs", "/k", "/k/cbm"]);
        assert_eq!(
            policy.scratch,
            [
                "/private/tmp/cbm-search-*",
                "/private/tmp/cbm-watcher-git-*",
                "/private/tmp/cbm-artifact-*",
            ]
        );
    }
}
