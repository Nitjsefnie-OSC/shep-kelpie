//! The hosts the MCP servers of the repo's plugins and `.mcp.json` reach
//!
//! A server runs inside the call's fence like anything else the call runs,
//! so a host it connects to has to be in `worker.allowed_domains`. Only a
//! server with a `url` names its host: what a command reaches cannot be told
//! from how it starts.

use std::collections::BTreeSet;

use super::{Here, Line};
use crate::settings::{NonBlank, Settings};

/// What a runner starting now would give the calls, as far as each server's host goes: nothing
/// where no server names one
///
/// A host left out never fails the run, since the server just cannot connect.
pub(super) fn hosts(subject: String, settings: &Settings, here: Here<'_>) -> Option<Line> {
    let plugins = crate::skills::inspect(
        &settings.git.checkout,
        settings.git.base(),
        here.kelpie_home,
        &settings.repo_setup,
    );
    let named: Vec<(&str, String)> = (plugins.iter())
        .flat_map(|p| p.mcp_servers())
        .filter_map(|(name, server)| Some((name.as_str(), crate::skills::host_of(server)?)))
        .collect();
    if named.is_empty() {
        return None;
    }
    let domains = &settings.worker.allowed_domains;
    let (covered, lacking): (Vec<_>, Vec<_>) = named
        .iter()
        .map(|(name, host)| (host, format!("{name} reaches {host}")))
        .partition(|(host, _)| allowed(host, domains));
    let said = |servers: Vec<(&String, String)>| {
        let said: BTreeSet<String> = servers.into_iter().map(|(_, said)| said).collect();
        said.into_iter().collect::<Vec<_>>().join("; ")
    };
    if lacking.is_empty() {
        let covered = said(covered);
        return Some(Line::ok(
            subject,
            format!("{covered}, which worker.allowed_domains covers"),
        ));
    }
    let lacking = said(lacking);
    Some(Line::unsure(
        subject,
        format!("{lacking}, which worker.allowed_domains lacks"),
        "add the host to `worker.allowed_domains` (and `researcher.allowed_domains` for the researcher), or the server cannot connect",
    ))
}

// Whether an allowed domain, such as `*.example.com` or `example.com:443`, covers `host`.
pub(super) fn allowed(host: &str, domains: &[NonBlank]) -> bool {
    domains.iter().any(|domain| {
        let domain = domain.as_str().trim().to_ascii_lowercase();
        let domain = domain
            .rsplit_once(':')
            .map_or(domain.as_str(), |(h, port)| {
                if port.chars().all(|c| c.is_ascii_digit()) {
                    h
                } else {
                    domain.as_str()
                }
            });
        match domain.strip_prefix("*.") {
            Some(suffix) => host.ends_with(&format!(".{suffix}")),
            None => host == domain,
        }
    })
}
