//! The web a researcher on goose has: `duckduckgo-websearch`, as a stdio MCP server
//!
//! goose v1.54.0 ships no search or fetch tool. The researcher's call names the
//! server as stdio server `kelpie-web` (`Runner::researcher_call`), `mcp`
//! passes it as a flag, and the hook lets `kelpie-web__search` and
//! `kelpie-web__fetch_content` through. Its fetch dials the page's host through
//! the sandbox's proxy, so `researcher.allowed_domains` bounds it. Its keyless
//! search POSTs to [`SEARCH_HOST`]. Node's `fetch` ignores the proxy without
//! `NODE_USE_ENV_PROXY=1`, so the call sets it. See `docs/facts/goose-as-an-agent.md`.

use std::ffi::OsString;

use crate::ports::{AgentCall, Policy, Role, Tools};
use crate::tools::GOOSE_WEB_SERVER;

/// The one host the server's search dials
///
/// It also pings `duckduckgo.com` after a search, which it ignores the failure of, so that host
/// is left out.
pub(super) const SEARCH_HOST: &str = "html.duckduckgo.com";

/// Whether `call` has the web: the researcher's, when its reach names the server
///
/// A researcher whose package is not installed names no server and reaches no search host.
pub(super) fn loaded(call: &AgentCall) -> bool {
    call.role == Role::Researcher
        && call.tools == Tools::Research
        && call.reach.mcp_servers.contains_key(GOOSE_WEB_SERVER)
}

/// The variables a call that has the web needs
///
/// The server's `fetch` is Node's, which ignores the sandbox's proxy without this.
pub(super) fn env(call: &AgentCall) -> Vec<(String, OsString)> {
    match loaded(call) {
        true => vec![("NODE_USE_ENV_PROXY".into(), "1".into())],
        false => Vec::new(),
    }
}

/// Adds the search host to the hosts `policy` reaches, for a call that has the web
///
/// The model's host stays denied: `deny_the_server` applies after this, and no list opens it.
pub(super) fn open(call: &AgentCall, policy: &mut Policy) {
    if loaded(call) && !policy.hosts.iter().any(|h| h == SEARCH_HOST) {
        policy.hosts.push(SEARCH_HOST.to_owned());
    }
}
