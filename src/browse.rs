//! What a worker's browser tool call may name
//!
//! The Playwright MCP server runs outside the sandbox, and its own fence
//! blocks only the `file:` scheme: `view-source:file://` read any file. The
//! bridge ([`crate::bridge`]) lets a URL through only when it is `http` or
//! `https` to one of the project's `browser.domains`, and refuses every file
//! name, since the server checks one before it writes. Screenshots come back
//! in the tool's reply instead. A turn's dev server ([`crate::ports::DevServers`])
//! is the one local address it may open, on its one port.

use serde_json::Value;

use crate::confine::Verdict;

/// The names of this machine that the dev server's port opens, and nothing else does
pub(crate) const LOCAL: [&str; 2] = ["localhost", "127.0.0.1"];

/// Judges the arguments of a browser tool call against the project's `domains`
/// and the port of the turn's dev server, if it has one
pub fn judge(arguments: &Value, domains: &[String], dev_port: Option<u16>) -> Verdict {
    let Some(arguments) = arguments.as_object() else {
        return Verdict::Refuse("kelpie cannot read this tool call's arguments".into());
    };
    // The server resolves a file name's symlinks, then writes later: a worktree
    // folder swapped for a link in between took a write outside it.
    if arguments.get("filename").is_some_and(|f| !f.is_null()) {
        return Verdict::Refuse(
            "the browser's tools save no file under kelpie: leave out `filename`, and a \
             screenshot comes back in the tool's reply"
                .into(),
        );
    }
    match arguments.get("url") {
        None | Some(Value::Null) => Verdict::Allow,
        Some(Value::String(url)) if allowed_url(url, domains, dev_port) => Verdict::Allow,
        Some(url) => Verdict::Refuse(format!(
            "{url} is not a browser domain of this project: the browser opens only http \
             and https URLs to {}",
            hosts_told(domains, dev_port)
        )),
    }
}

// The hosts a refusal names: the domains, and the dev server's origin where there is one.
fn hosts_told(domains: &[String], dev_port: Option<u16>) -> String {
    let local = dev_port.map(|port| format!("http://localhost:{port}"));
    let told: Vec<String> = domains.iter().cloned().chain(local).collect();
    match told.as_slice() {
        [] => "no host, since the project lists none".to_owned(),
        _ => told.join(", "),
    }
}

/// Whether `url` is `http` or `https` to one of `domains`, or to `localhost` or
/// `127.0.0.1` on `dev_port`
///
/// A domain is a host, or `*.` and a parent for its subdomains. A domain's
/// port is read but not held to anything, since the browser's resolver cannot
/// hold a host to one port either: the proxy does. The dev server's address
/// passes on its one port only. Anything this cannot read as such a URL is
/// refused: another scheme, a user name, an IPv6 host, an encoded host, or a
/// character a browser drops.
pub fn allowed_url(url: &str, domains: &[String], dev_port: Option<u16>) -> bool {
    let lower = url.to_ascii_lowercase();
    let (default, rest) = match (
        lower.strip_prefix("http://"),
        lower.strip_prefix("https://"),
    ) {
        (Some(rest), _) => (80, rest),
        (None, Some(rest)) => (443, rest),
        (None, None) => return false,
    };
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, port.parse::<u16>().ok())
        }
        Some(_) => return false,
        None => (authority, Some(default)),
    };
    let host = host.strip_suffix('.').unwrap_or(host);
    let plain = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
    if host.is_empty() || !host.chars().all(plain) {
        return false;
    }
    if LOCAL.contains(&host) {
        return dev_port.is_some() && port == dev_port;
    }
    domains
        .iter()
        .any(|domain| names(&domain.to_ascii_lowercase(), host))
}

/// Whether `domain`, as the project lists it in lower case, names `host`
pub(crate) fn names(domain: &str, host: &str) -> bool {
    match domain.strip_prefix("*.") {
        Some(parent) => host.ends_with(&format!(".{parent}")),
        None => host == domain,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn domains() -> Vec<String> {
        vec!["pokemon-go-api.github.io".into(), "*.leekduck.com".into()]
    }

    #[test]
    fn http_and_https_to_a_listed_domain_or_its_wildcard_pass() {
        for url in [
            "http://pokemon-go-api.github.io/",
            "https://pokemon-go-api.github.io/api/raidboss.json",
            "https://cdn.leekduck.com/a.png?x=1#y",
            "https://a.b.leekduck.com/",
            "HTTPS://POKEMON-GO-API.GITHUB.IO/",
            "https://pokemon-go-api.github.io./",
            "https://pokemon-go-api.github.io:8443/app",
        ] {
            assert!(allowed_url(url, &domains(), None), "{url}");
        }
    }

    // The review's canary read a file through `view-source:file://`.
    #[test]
    fn every_other_scheme_and_every_other_host_is_refused() {
        for url in [
            "view-source:file:///Users/me/.kelpie/settings.toml",
            "file:///etc/hosts",
            "view-source:https://pokemon-go-api.github.io/",
            "javascript:alert(1)",
            "data:text/html,<p>",
            "about:blank",
            "chrome://settings",
            "ws://pokemon-go-api.github.io/",
            " https://pokemon-go-api.github.io/",
            "https://example.com/",
            "https://leekduck.com/",
            "https://evilpokemon-go-api.github.io/",
            "https://pokemon-go-api.github.io@evil.example/",
            "https://evil.example\\@pokemon-go-api.github.io/",
            "https://pokemon-go-api.github.io.evil.example/",
            "https://%70okemon-go-api.github.io/",
            "https://pokemon-go-api.github.io:443x/",
            "https://staging.example.com/",
            "http://localhost:3000/",
            "http://127.0.0.1:3000/",
            "http://[::1]:3000/",
            "http://192.168.1.10:3000/",
        ] {
            assert!(!allowed_url(url, &domains(), None), "{url}");
        }
    }

    #[test]
    fn a_project_that_lists_no_domain_opens_nothing() {
        assert!(!allowed_url("https://example.com/", &[], None));
        assert!(!allowed_url("http://localhost:3000/", &[], None));
    }

    #[test]
    fn the_dev_servers_address_passes_on_its_one_port_and_no_other_local_address_does() {
        let on = |url: &str| allowed_url(url, &domains(), Some(5173));
        for url in [
            "http://localhost:5173/",
            "http://localhost:5173/app?x=1#y",
            "http://LOCALHOST:5173/",
            "http://127.0.0.1:5173/",
            "https://localhost:5173/",
            "http://localhost.:5173/",
        ] {
            assert!(on(url), "{url}");
        }
        for url in [
            "http://localhost/",
            "http://localhost:80/",
            "http://localhost:5174/",
            "http://localhost:51730/",
            "http://localhost:5173x/",
            "http://localhost:99999/",
            "http://127.0.0.1:22/",
            "http://127.0.0.2:5173/",
            "http://0.0.0.0:5173/",
            "http://[::1]:5173/",
            "http://app.localhost:5173/",
            "http://localhost.evil.example:5173/",
            "http://localhost@evil.example:5173/",
            "http://evil.example:5173/",
            "ws://localhost:5173/",
            "file://localhost:5173/etc/hosts",
        ] {
            assert!(!on(url), "{url}");
        }
        // The listed domains still pass beside it.
        assert!(on("https://cdn.leekduck.com/a.png"));
    }

    #[test]
    fn a_turn_without_a_dev_server_opens_no_local_address() {
        for url in [
            "http://localhost:5173/",
            "http://127.0.0.1:5173/",
            "http://localhost/",
        ] {
            assert!(!allowed_url(url, &domains(), None), "{url}");
        }
    }

    #[test]
    fn a_refusal_names_the_dev_servers_address_when_the_turn_has_one() {
        let Verdict::Refuse(why) = judge(
            &json!({ "url": "http://localhost:9/" }),
            &domains(),
            Some(5173),
        ) else {
            panic!("a wrong port was let through");
        };
        assert!(why.contains("http://localhost:5173"), "{why}");
        let Verdict::Refuse(why) = judge(&json!({ "url": "http://localhost:9/" }), &[], None)
        else {
            panic!("localhost was let through");
        };
        assert!(why.contains("no host"), "{why}");
        assert_eq!(
            judge(&json!({ "url": "http://localhost:5173/" }), &[], Some(5173)),
            Verdict::Allow
        );
    }

    #[test]
    fn navigate_and_a_new_tab_are_held_to_the_domains() {
        let navigate = |url: &str| judge(&json!({ "url": url }), &domains(), None);
        assert_eq!(
            navigate("https://pokemon-go-api.github.io/"),
            Verdict::Allow
        );
        let Verdict::Refuse(why) = navigate("view-source:file:///etc/hosts") else {
            panic!("view-source was let through");
        };
        assert!(why.contains("not a browser domain"), "{why}");
        let tab = |action: &str, url: &str| {
            judge(&json!({ "action": action, "url": url }), &domains(), None)
        };
        assert!(matches!(
            tab("new", "file:///etc/hosts"),
            Verdict::Refuse(_)
        ));
        assert_eq!(
            judge(&json!({ "action": "list" }), &domains(), None),
            Verdict::Allow
        );
    }

    // The review wrote outside the worktree by racing a folder against a link.
    #[test]
    fn any_file_name_is_refused_and_a_null_one_is_not_a_name() {
        for filename in [
            "shot.png",
            "d/x.json",
            ".claude/settings.local.json",
            "/tmp/x",
        ] {
            let verdict = judge(&json!({ "filename": filename }), &domains(), None);
            assert!(matches!(verdict, Verdict::Refuse(_)), "{filename}");
        }
        assert_eq!(
            judge(&json!({ "type": "png" }), &domains(), None),
            Verdict::Allow
        );
        assert_eq!(
            judge(&json!({ "filename": null }), &domains(), None),
            Verdict::Allow
        );
    }

    #[test]
    fn arguments_that_are_not_an_object_are_refused() {
        for arguments in [json!("x"), json!([]), json!(3), Value::Null] {
            assert!(
                matches!(judge(&arguments, &domains(), None), Verdict::Refuse(_)),
                "{arguments}"
            );
        }
    }
}
