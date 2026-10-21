// A goose researcher's fetch, tried under the real sandbox runtime with the policy the adapter
// builds. `curl` stands in for the web server: inside the sandbox, the server's fetch and a
// command reach the network alike, through the proxy the sandbox sets.

use std::process::{Command, Output};

use super::tests::{FRESH_ID, URL, World, id};
use super::web::SEARCH_HOST;
use super::*;
use crate::adapters::SandboxRuntime;
use crate::ports::Sandbox;
use crate::tools::{GOOSE_WEB_SERVER, Tools as KelpieTools};

const NEEDS: &str = "needs kelpie's tools: KELPIE_TOOLS=<dir> from `shep kelpie tools install`";

// A researcher's fetch is the server dialling the page's host through the sandbox's proxy, which
// `curl` does the same way, so the proxy's answer is the server's: a listed host is let through,
// any other is refused by name. Plain `http://` URLs make the proxy answer in its own words.
#[test]
#[ignore = "needs kelpie's tools, and runs the real sandbox"]
fn a_researchers_fetch_reaches_only_the_hosts_its_policy_lists() {
    let world = World::new();
    let mut call = world.fenced(URL, Path::new("/k/kelpie"), Session::New(id(FRESH_ID)));
    call.role = Role::Researcher;
    call.tools = Tools::Research;
    call.reach.mcp_servers = [(
        GOOSE_WEB_SERVER.to_owned(),
        serde_json::json!({
            "type": "stdio",
            "command": "node",
            "args": [world.path("tools/node_modules/duckduckgo-websearch/build/index.js")],
        }),
    )]
    .into();
    call.reach.fence.as_mut().unwrap().hosts = vec!["example.com".to_owned()];
    let upstream = GooseCli::on(&Base::default())
        .upstream(server(&call).unwrap())
        .unwrap();
    let forwarder = Forwarder::open(call.settings.parent().unwrap(), upstream).unwrap();
    let refused = |url: &str| {
        let reply = said(&curl(&world, &call, &forwarder, &["--noproxy", "", url]));
        reply.contains("blocked by network allowlist") && reply.ends_with("[403]")
    };
    // The project's host, and the one search host: the proxy let the request out, so the host
    // answered with a status of its own (`curl` prints `[000]` when nothing answered).
    let reached = |url: &str| {
        let reply = said(&curl(&world, &call, &forwarder, &["--noproxy", "", url]));
        let status = reply.rsplit('[').next().and_then(|s| s.strip_suffix(']'));
        let status: u16 = status.and_then(|s| s.parse().ok()).unwrap_or(0);
        !reply.contains("blocked by network allowlist") && (100..400).contains(&status)
    };
    assert!(
        reached("http://example.com/"),
        "example.com was not reached"
    );
    assert!(
        reached(&format!("http://{SEARCH_HOST}/")),
        "the search host was not reached"
    );
    // Any other, a host a fetch could be sent to included.
    for url in [
        "http://example.org/",
        "http://github.com/",
        "http://r.jina.ai/",
        "http://localhost/",
    ] {
        assert!(refused(url), "{url} was not refused");
    }
}

// `curl`'s arguments, run whole inside the sandbox with the proxy settings the sandbox gives it.
// `env` first, since the sandbox sets its own `no_proxy`.
fn curl(world: &World, call: &AgentCall, forwarder: &Forwarder, args: &[&str]) -> Output {
    let tools = KelpieTools::at(std::env::var("KELPIE_TOOLS").expect(NEEDS).into());
    let upstream = GooseCli::on(&Base::default())
        .upstream(server(call).unwrap())
        .unwrap();
    let policy = policy(
        call,
        &Files::of(&call.settings),
        &upstream,
        &forwarder.socket,
        &[],
    );
    let mut command = Command::new("env");
    command
        .args([
            "no_proxy=localhost",
            "NO_PROXY=localhost",
            "curl",
            "-s",
            "-m",
            "10",
        ])
        .args(["-w", " [%{http_code}]"])
        .args(args)
        .current_dir(world.path("wt"));
    let mut wrapped = SandboxRuntime::new(tools)
        .wrap(&policy, &world.path("settings.sandbox.json"), &command)
        .unwrap();
    wrapped.output().unwrap()
}

fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
