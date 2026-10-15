//! The MCP servers each call was given and its tool calls to them, read from
//! its transcript once it ends

use super::{Family, Label, Recorder};
use crate::ports::SeriesKind::Counter;

const SERVERS: Family = Family {
    name: "kelpie_mcp_servers_total",
    kind: Counter,
    help: "MCP servers given to the calls, by whether each connected or failed to.",
};
const TOOL_CALLS: Family = Family {
    name: "kelpie_mcp_tool_calls_total",
    kind: Counter,
    help: "Tool calls the calls made to each MCP server.",
};

/// Kelpie's own MCP servers, each its own `server` label. A server the
/// project names is `repo`, so the labels stay few.
pub(crate) const OWN_SERVERS: [&str; 2] = [crate::adapters::mcp::SERVER, crate::bridge::SERVER];

impl Recorder {
    /// Counts the MCP server `name`, given to a call, which `connected` or not
    pub fn mcp_server(&self, name: &str, connected: bool) {
        let outcome = if connected { "connected" } else { "failed" };
        let labels = vec![server(name), ("outcome", outcome.to_owned())];
        self.add(&SERVERS, labels, 1.0);
    }

    /// Adds `count` tool calls to the MCP server `name`
    pub fn mcp_tool_calls(&self, name: &str, count: u64) {
        #[expect(clippy::cast_precision_loss, reason = "a call's tool calls")]
        self.add(&TOOL_CALLS, vec![server(name)], count as f64);
    }
}

fn server(name: &str) -> Label {
    let own = OWN_SERVERS.contains(&name);
    ("server", if own { name } else { "repo" }.to_owned())
}
