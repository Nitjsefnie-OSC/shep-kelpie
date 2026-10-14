//! A researcher's web on goose: the search host, Node's proxy variable, and nothing else
//!
//! The server entry is what `Runner::researcher_call` puts in the reach, so these tests build the
//! reach the way it does and read back what the adapter makes of it.

use serde_json::json;

use super::tests::{FRESH_ID, URL, World, id, strings};
use super::web::{SEARCH_HOST, SERVER};
use super::*;

const ROLES: [Role; 6] = [
    Role::Worker,
    Role::Reviewer,
    Role::IssueWriter,
    Role::Pm,
    Role::Advisor,
    Role::Researcher,
];

const KINDS: [Tools; 8] = [
    Tools::Work,
    Tools::Review,
    Tools::Answer,
    Tools::Issues,
    Tools::Pm,
    Tools::Retro,
    Tools::Advise,
    Tools::Research,
];

// The entry a researcher's reach holds when the package is installed.
fn entry(w: &World) -> serde_json::Value {
    json!({
        "type": "stdio",
        "command": "node",
        "args": [w.path("kelpie/tools/node_modules/duckduckgo-websearch/build/index.js")],
    })
}

// A call of `role` and `tools` whose reach names the web server, behind a fence that allows `hosts`.
fn call(w: &World, role: Role, tools: Tools, hosts: &[&str]) -> AgentCall {
    let mut call = w.fenced(URL, Path::new("/k/kelpie"), Session::New(id(FRESH_ID)));
    call.role = role;
    call.tools = tools;
    call.reach.mcp_servers = [(SERVER.to_owned(), entry(w))].into();
    let fence = call.reach.fence.as_mut().unwrap();
    fence.hosts = hosts.iter().map(|h| (*h).to_owned()).collect();
    call
}

fn made(call: &AgentCall) -> (Vec<String>, Policy) {
    let upstream = ClaudeCli::default()
        .goose()
        .upstream(server(call).unwrap())
        .unwrap();
    let files = Files::of(&call.settings);
    let env = env_args(call, server(call).unwrap(), &files, &upstream);
    let env = env.into_iter().map(|v| v.into_string().unwrap()).collect();
    let socket = Path::new("/k/forwarder.sock");
    (env, policy(call, &files, &upstream, socket, &[]))
}

#[test]
fn a_researcher_call_gets_the_search_host_and_node_s_proxy_variable_and_no_other_call_does() {
    let w = World::new();
    for role in ROLES {
        for tools in KINDS {
            let call = call(&w, role, tools, &["example.com"]);
            let (env, policy) = made(&call);
            let has_web = role == Role::Researcher && tools == Tools::Research;
            let what = format!("{role:?} {tools:?}");
            assert_eq!(
                env.iter().any(|v| v == "NODE_USE_ENV_PROXY=1"),
                has_web,
                "{what}"
            );
            assert_eq!(
                policy.hosts.iter().any(|h| h == SEARCH_HOST),
                has_web,
                "{what}"
            );
        }
    }

    // A researcher whose reach names no server (the package is not installed) gets neither.
    let mut bare = call(&w, Role::Researcher, Tools::Research, &["example.com"]);
    bare.reach.mcp_servers.clear();
    let (env, policy) = made(&bare);
    assert!(!env.iter().any(|v| v.starts_with("NODE_USE_ENV_PROXY=")));
    assert_eq!(policy.hosts, ["example.com"]);
}

#[test]
fn the_researchers_policy_reaches_its_allowed_domains_and_the_search_host_and_nothing_else() {
    let w = World::new();
    let call = call(
        &w,
        Role::Researcher,
        Tools::Research,
        &["example.com", "docs.rs"],
    );
    let (_, policy) = made(&call);
    assert_eq!(policy.hosts, ["example.com", "docs.rs", SEARCH_HOST]);
    // The model's host and this machine stay denied.
    assert_eq!(
        policy.denied_hosts,
        ["192.0.2.9", "localhost", "127.0.0.1", "[::1]"]
    );
    assert_eq!(policy.denied_addresses, ["192.0.2.9"]);

    // The project listing the search host does not add it twice.
    let listed = call_with(&w, &[SEARCH_HOST]);
    assert_eq!(made(&listed).1.hosts, [SEARCH_HOST]);

    // A project that lists the model's host does not open it: the denial is applied after.
    let model = call_with(&w, &["192.0.2.9", "localhost"]);
    let (_, policy) = made(&model);
    assert!(policy.denied_hosts.iter().any(|h| h == "192.0.2.9"));
    assert!(policy.denied_hosts.iter().any(|h| h == "localhost"));
}

fn call_with(w: &World, hosts: &[&str]) -> AgentCall {
    call(w, Role::Researcher, Tools::Research, hosts)
}

#[test]
fn the_web_server_names_no_key_and_reads_nothing_of_the_home_folder() {
    let w = World::new();
    let with = call_with(&w, &["example.com"]);
    let mut without = with.clone();
    without.reach.mcp_servers.clear();
    let ((env, policy), (plain_env, plain_policy)) = (made(&with), made(&without));

    // Against the same researcher without the web, the adapter's whole change is the search host
    // and the proxy variable: no file read, written or hidden, and no other variable. goose runs
    // `auto` once a server is loaded, so that is the one value that differs.
    assert_eq!(policy.read, plain_policy.read);
    assert_eq!(policy.write, plain_policy.write);
    assert_eq!(policy.no_read, plain_policy.no_read);
    let added: Vec<&String> = env.iter().filter(|v| !plain_env.contains(v)).collect();
    assert_eq!(added, ["GOOSE_MODE=auto", "NODE_USE_ENV_PROXY=1"]);

    // The flag that starts the server holds a command and the server file, and no variable.
    let argv = strings(&with);
    let at = argv.iter().position(|a| a == "--with-extension").unwrap();
    let file = w.path("kelpie/tools/node_modules/duckduckgo-websearch/build/index.js");
    assert_eq!(argv[at + 1], format!("{SERVER}:node {}", file.display()));

    // No key, token or secret in the flag or in what the web adds to the environment.
    let shown = format!("{} {added:?}", argv[at + 1]).to_ascii_lowercase();
    for word in ["key", "token", "secret", "password", "home"] {
        assert!(!shown.contains(word), "{word} in {shown}");
    }
}
