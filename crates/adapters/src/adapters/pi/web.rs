//! The web a researcher on pi has: `pi-web-access`, loaded from kelpie's tools
//!
//! pi has no web tool of its own. The researcher's call loads the package
//! with `--extension`, which `tools install` put under kelpie's tools at an
//! exact version, and lists three of its tools. Everything the package could
//! reach beyond the page host and one search host is turned off in a config
//! kelpie writes into the call's pi home, which the call cannot change, and
//! the sandbox's allowlist holds the rest: its fetch dials the page's own
//! host through the sandbox's proxy, so `researcher.allowed_domains` bounds
//! it, and its keyless search dials `mcp.exa.ai` alone. No other call loads
//! the package or reaches that host. What was measured is in
//! `docs/facts/the-second-harness.md`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::ports::{AgentCall, AgentError, Policy, Role, Tools};
use crate::tools::PI_WEB_ACCESS;

/// pi's names for the package's tools the researcher is given, besides `read`,
/// `grep`, `find` and `ls`
///
/// `get_search_content` reads back a result the other two stored, which they
/// bound. `source_check` is the package's fourth, and the config turns it off.
pub(super) const TOOLS: [&str; 3] = ["web_search", "fetch_content", "get_search_content"];

/// The researcher's `--tools`: its reading tools and [`TOOLS`]
pub(super) const RESEARCH_TOOLS: &str =
    "read,grep,find,ls,web_search,fetch_content,get_search_content";

/// The one host the package's search dials with no key
pub(super) const SEARCH_HOST: &str = "mcp.exa.ai";

/// What moves the package's Exa search off the keyless host
const EXA_VARS: [&str; 2] = ["EXA_API_KEY", "EXA_BASE_URL"];

/// The package's config, which it reads from the call's pi home
const CONFIG: &str = "web-search.json";

/// The web package's folders `call` names: the extensions called `pi-web-access`
///
/// An extension of any other name is not the pinned package, so nothing here
/// opens the web for it.
fn packages(call: &AgentCall) -> impl Iterator<Item = &PathBuf> {
    (call.reach.extensions.iter()).filter(|e| e.file_name().is_some_and(|n| n == PI_WEB_ACCESS))
}

/// Whether `call` has the web: the researcher's, when its reach names the package
pub(super) fn loaded(call: &AgentCall) -> bool {
    call.role == Role::Researcher
        && call.tools == Tools::Research
        && packages(call).next().is_some()
}

/// The flags that load the package, for a call that has the web
pub(super) fn flags(call: &AgentCall) -> Vec<OsString> {
    if !loaded(call) {
        return Vec::new();
    }
    packages(call)
        .flat_map(|package| ["--extension".into(), package.clone().into_os_string()])
        .collect()
}

/// The variables a call that has the web needs
///
/// The package caches what it fetched. Without this it does so in the pi
/// home, which the call may not write.
pub(super) fn env(call: &AgentCall, scratch: &Path) -> Vec<(String, OsString)> {
    match loaded(call) {
        true => vec![("PI_WEB_ACCESS_CACHE_ROOT".into(), scratch.into())],
        false => Vec::new(),
    }
}

/// The variables a call that has the web must not inherit
///
/// The package swaps its keyless Exa host for another when kelpie's own
/// environment holds a key or a base URL for Exa, and the allowlist holds only
/// the keyless one. The config lists no other provider, so these are the two
/// that reach it.
pub(super) fn unset(call: &AgentCall) -> Vec<&'static str> {
    match loaded(call) {
        true => EXA_VARS.to_vec(),
        false => Vec::new(),
    }
}

/// Adds what a call that has the web may read and reach to `policy`
///
/// The package imports its dependencies from the folder it sits in, where
/// npm put them beside it, so the call reads that folder.
pub(super) fn open(call: &AgentCall, policy: &mut Policy) {
    if !loaded(call) {
        return;
    }
    for package in packages(call) {
        policy.read.push(package.clone());
        policy.read.extend(package.parent().map(Path::to_path_buf));
    }
    if !policy.hosts.iter().any(|h| h == SEARCH_HOST) {
        policy.hosts.push(SEARCH_HOST.to_owned());
    }
}

/// Writes the package's config into `home` for a call that has the web, and
/// removes the one an earlier call left for any other
///
/// # Errors
///
/// [`AgentError::Setup`] when the file cannot be written or removed.
pub(super) fn write_config(call: &AgentCall, home: &Path) -> Result<(), AgentError> {
    let path = home.join(CONFIG);
    let setup = |e: std::io::Error| {
        AgentError::Setup(format!("cannot write {}: {}", path.display(), e.kind()))
    };
    if loaded(call) {
        let text = serde_json::to_string_pretty(&config()).expect("a config is JSON");
        return std::fs::write(&path, text).map_err(setup);
    }
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(setup(e)),
        _ => Ok(()),
    }
}

// What the package may do, and nothing that needs a key or another host.
fn config() -> Value {
    json!({
        // The default registers a tool that turns the web on and sets the
        // active tools itself, which would override `--tools`.
        "toolActivation": "eager",
        // It resolves a page's name itself before it fetches. The sandbox
        // gives the call no DNS, so it leaves that to the proxy.
        "ssrf": { "trustEnvProxy": true },
        "tools": { "sourceCheck": { "enabled": false } },
        // Exa through its keyless endpoint, the one search host.
        "webSearch": { "allowedProviders": ["exa"] },
        // `answer` sends a page to a model service.
        "fetch": { "allowedModes": ["readable", "raw"] },
        // A page is fetched from its own host, never through a reader service.
        "fetchRouting": { "providers": ["http"], "allowRemoteHostedProviders": false },
        "githubClone": { "enabled": false },
        "workflow": "none",
        "autoOpenBrowser": false,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::tests::{FRESH, FRESH_ID, URL, World, id, stand_in, strings};
    use super::*;
    use crate::adapters::{Base, PiCli};
    use crate::ports::{Agents, Session};

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

    fn package(w: &World) -> PathBuf {
        w.path("kelpie/tools/node_modules/pi-web-access")
    }

    // A researcher's call whose reach names the package.
    fn researcher(w: &World) -> AgentCall {
        let mut call = w.call(URL, Role::Researcher, Session::New(id(FRESH_ID)));
        call.tools = Tools::Research;
        call.reach.extensions = vec![package(w)];
        call
    }

    #[test]
    fn a_researcher_call_loads_the_package_and_lists_its_tools() {
        let w = World::new();
        let argv = strings(&researcher(&w));
        let package = package(&w);
        let at = argv.iter().position(|a| a == "--extension").unwrap();
        assert_eq!(argv[at + 1], package.to_str().unwrap());
        let at = argv.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(
            argv[at + 1],
            "read,grep,find,ls,web_search,fetch_content,get_search_content"
        );
        assert!(!argv.iter().any(|a| a == "--no-tools"), "{argv:?}");
        // The web is the package's alone: nothing that writes or runs.
        for tool in ["bash", "write", "edit", "source_check", "web_enable"] {
            assert!(!argv[at + 1].split(',').any(|t| t == tool), "{tool}");
        }
    }

    #[test]
    fn the_researchers_tools_are_the_reading_ones_and_the_packages() {
        let reading = super::super::tool_names(Tools::Research, false, false, false).unwrap();
        assert_eq!(RESEARCH_TOOLS, format!("{reading},{}", TOOLS.join(",")));
    }

    #[test]
    fn a_researcher_whose_package_is_missing_reads_only() {
        let w = World::new();
        let mut call = researcher(&w);
        call.reach.extensions.clear();
        let argv = strings(&call);
        let at = argv.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(argv[at + 1], "read,grep,find,ls");
        assert!(!argv.iter().any(|a| a == "--extension"), "{argv:?}");
    }

    #[test]
    fn a_researcher_with_some_other_extension_gets_no_web() {
        let w = World::new();
        let mut call = researcher(&w);
        call.reach.extensions = vec![w.path("kelpie/tools/node_modules/some-other-extension")];
        assert!(!loaded(&call));
        let argv = strings(&call);
        assert!(!argv.iter().any(|a| a == "--extension"), "{argv:?}");
        let at = argv.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(argv[at + 1], "read,grep,find,ls");
        assert!(flags(&call).is_empty());
        let mut policy = Policy::default();
        open(&call, &mut policy);
        assert!(policy.hosts.is_empty() && policy.read.is_empty());
        let pi = stand_in(&w, FRESH);
        pi.prepare(&call).unwrap();
        assert!(!w.path("settings.pi/web-search.json").exists());
    }

    #[test]
    fn no_other_call_loads_the_package_or_gets_its_host() {
        let w = World::new();
        let upstream = PiCli::on(&Base::default())
            .upstream(
                super::super::server(&w.call(URL, Role::Reviewer, Session::New(id(FRESH_ID))))
                    .unwrap(),
            )
            .unwrap();
        let socket = w.path("forwarder.sock");
        for role in ROLES {
            for tools in KINDS {
                let mut call = w.call(URL, role, Session::New(id(FRESH_ID)));
                call.tools = tools;
                call.reach.extensions = vec![package(&w)];
                let argv = strings(&call);
                let files = super::super::Files::of(&call);
                let policy = super::super::policy(&call, &files, &upstream, &socket, &[]);
                let env = super::super::env_args(&call, &files);
                let env: Vec<_> = env.iter().map(|v| v.to_string_lossy()).collect();
                let has_web = role == Role::Researcher && tools == Tools::Research;
                let what = format!("{role:?} {tools:?}");
                assert_eq!(argv.iter().any(|a| a == "--extension"), has_web, "{what}");
                assert_eq!(
                    argv.iter().any(|a| a.contains("web_search")),
                    has_web,
                    "{what}"
                );
                assert_eq!(
                    policy.hosts.iter().any(|h| h == SEARCH_HOST),
                    has_web,
                    "{what}"
                );
                assert_eq!(policy.read.contains(&package(&w)), has_web, "{what}");
                assert_eq!(
                    env.iter()
                        .any(|v| v.starts_with("PI_WEB_ACCESS_CACHE_ROOT=")),
                    has_web,
                    "{what}"
                );
            }
        }
    }

    #[test]
    fn the_config_lands_in_the_call_s_pi_home_and_names_no_key() {
        let w = World::new();
        let pi = stand_in(&w, FRESH);
        let call = researcher(&w);
        pi.prepare(&call).unwrap();
        let path = w.path("settings.pi/web-search.json");
        let text = std::fs::read_to_string(&path).unwrap();
        let config: Value = serde_json::from_str(&text).unwrap();

        // Only the package's own pick of tools, search host and fetch.
        assert_eq!(config["toolActivation"], "eager");
        assert_eq!(config["webSearch"]["allowedProviders"], json!(["exa"]));
        assert_eq!(config["fetchRouting"]["providers"], json!(["http"]));
        assert_eq!(config["fetchRouting"]["allowRemoteHostedProviders"], false);
        assert_eq!(config["fetch"]["allowedModes"], json!(["readable", "raw"]));
        assert_eq!(config["githubClone"]["enabled"], false);
        assert_eq!(config["tools"]["sourceCheck"]["enabled"], false);

        // No key, token or secret of any provider, by name or by value.
        let lower = text.to_ascii_lowercase();
        for word in ["key", "token", "secret", "password"] {
            assert!(!lower.contains(word), "{word} in {text}");
        }

        // The call may read its home, not write the file.
        let policy = {
            let upstream = PiCli::on(&Base::default())
                .upstream(super::super::server(&call).unwrap())
                .unwrap();
            let files = super::super::Files::of(&call);
            super::super::policy(&call, &files, &upstream, &w.path("f.sock"), &[])
        };
        assert!(policy.read.contains(&w.path("settings.pi")));
        assert!(!policy.write.contains(&w.path("settings.pi")));
        assert!(!policy.write.contains(&path));

        // The next call on the same home, any other, finds no config.
        let mut next = call.clone();
        next.role = Role::Advisor;
        next.tools = Tools::Advise;
        pi.prepare(&next).unwrap();
        assert!(!path.exists());

        // A researcher with no package has none either.
        let mut bare = call.clone();
        bare.reach.extensions.clear();
        pi.prepare(&bare).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn a_researchers_call_unsets_what_would_move_the_search_off_its_host() {
        let w = World::new();
        let call = researcher(&w);
        let files = super::super::Files::of(&call);
        let args = super::super::env_args(&call, &files);
        let args: Vec<_> = args.iter().map(|a| a.to_string_lossy()).collect();
        // `env` takes its options before the first NAME=VALUE.
        assert_eq!(
            args[..4],
            ["-u", "EXA_API_KEY", "-u", "EXA_BASE_URL"],
            "{args:?}"
        );
        let first = args.iter().position(|a| a.contains('=')).unwrap();
        assert!(first >= 4, "{args:?}");

        let mut other = call.clone();
        other.role = Role::Advisor;
        other.tools = Tools::Advise;
        let args = super::super::env_args(&other, &files);
        assert!(!args.iter().any(|a| a == "-u"), "{args:?}");
    }

    #[test]
    fn the_package_caches_in_the_call_s_scratch_folder() {
        let w = World::new();
        let call = researcher(&w);
        let scratch = super::super::Files::of(&call).scratch;
        assert_eq!(
            env(&call, &scratch),
            [(
                "PI_WEB_ACCESS_CACHE_ROOT".to_owned(),
                scratch.clone().into()
            )]
        );
    }
}
