//! What a worker's browser tool call may name
//!
//! The Playwright MCP server runs outside the sandbox, and its own fence
//! blocks only the `file:` scheme: `view-source:file://` read any file. The
//! bridge ([`crate::bridge`]) lets a URL through only when it is `http` or
//! `https` to one of the project's `browser.domains`, and refuses every file
//! name, since the server checks one before it writes. Screenshots come back
//! in the tool's reply instead.

use serde_json::Value;

use crate::confine::Verdict;

/// Judges the arguments of a browser tool call against the project's `domains`
pub fn judge(arguments: &Value, domains: &[String]) -> Verdict {
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
        Some(Value::String(url)) if allowed_url(url, domains) => Verdict::Allow,
        Some(url) => Verdict::Refuse(format!(
            "{url} is not a browser domain of this project: the browser opens only http \
             and https URLs to {}",
            match domains {
                [] => "no host, since the project lists none".to_owned(),
                _ => domains.join(", "),
            }
        )),
    }
}

/// Whether `url` is `http` or `https` to one of `domains`
///
/// A domain is a host, or `*.` and a parent for its subdomains, with a `:port`
/// after either to hold the URL to that port. Anything this cannot read as
/// such a URL is refused: another scheme, a user name, an IPv6 host, an
/// encoded host, or a character a browser drops.
pub fn allowed_url(url: &str, domains: &[String]) -> bool {
    let lower = url.to_ascii_lowercase();
    let Some(rest) = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port))
        }
        Some(_) => return false,
        None => (authority, None),
    };
    let host = host.strip_suffix('.').unwrap_or(host);
    let plain = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
    if host.is_empty() || !host.chars().all(plain) {
        return false;
    }
    domains
        .iter()
        .any(|domain| names(&domain.to_ascii_lowercase(), host, port))
}

// Whether `domain`, as the project lists it, names `host` and `port` of a URL.
fn names(domain: &str, host: &str, port: Option<&str>) -> bool {
    let (domain, wanted) = match domain.rsplit_once(':') {
        Some((domain, wanted)) if wanted.chars().all(|c| c.is_ascii_digit()) => {
            (domain, Some(wanted))
        }
        _ => (domain, None),
    };
    if wanted.is_some_and(|wanted| Some(wanted) != port) {
        return false;
    }
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
        vec![
            "pokemon-go-api.github.io".into(),
            "*.leekduck.com".into(),
            "staging.example.com:8443".into(),
        ]
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
            "https://staging.example.com:8443/app",
        ] {
            assert!(allowed_url(url, &domains()), "{url}");
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
            "https://staging.example.com:9000/",
            "http://localhost:3000/",
            "http://127.0.0.1:3000/",
            "http://[::1]:3000/",
            "http://192.168.1.10:3000/",
        ] {
            assert!(!allowed_url(url, &domains()), "{url}");
        }
    }

    #[test]
    fn a_project_that_lists_no_domain_opens_nothing() {
        assert!(!allowed_url("https://example.com/", &[]));
        assert!(!allowed_url("http://localhost:3000/", &[]));
    }

    #[test]
    fn navigate_and_a_new_tab_are_held_to_the_domains() {
        let navigate = |url: &str| judge(&json!({ "url": url }), &domains());
        assert_eq!(
            navigate("https://pokemon-go-api.github.io/"),
            Verdict::Allow
        );
        let Verdict::Refuse(why) = navigate("view-source:file:///etc/hosts") else {
            panic!("view-source was let through");
        };
        assert!(why.contains("not a browser domain"), "{why}");
        let tab =
            |action: &str, url: &str| judge(&json!({ "action": action, "url": url }), &domains());
        assert!(matches!(
            tab("new", "file:///etc/hosts"),
            Verdict::Refuse(_)
        ));
        assert_eq!(
            judge(&json!({ "action": "list" }), &domains()),
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
            let verdict = judge(&json!({ "filename": filename }), &domains());
            assert!(matches!(verdict, Verdict::Refuse(_)), "{filename}");
        }
        assert_eq!(judge(&json!({ "type": "png" }), &domains()), Verdict::Allow);
        assert_eq!(
            judge(&json!({ "filename": null }), &domains()),
            Verdict::Allow
        );
    }

    #[test]
    fn arguments_that_are_not_an_object_are_refused() {
        for arguments in [json!("x"), json!([]), json!(3), Value::Null] {
            assert!(
                matches!(judge(&arguments, &domains()), Verdict::Refuse(_)),
                "{arguments}"
            );
        }
    }
}
