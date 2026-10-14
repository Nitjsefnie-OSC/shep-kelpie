//! `[app.dogs.kelpie.browser]`: whether a worker drives a browser to see the UI it builds

use schemars::JsonSchema;
use serde::de::Error;
use serde::{Deserialize, Deserializer};

use super::{NonBlank, domains};

/// Whether the project's workers are given a browser outside their sandbox
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Browser")]
pub struct Browser {
    /// Whether a worker's turn is given the Playwright MCP server, which
    /// kelpie runs outside the worker's sandbox and filters. macOS only. `shep
    /// kelpie tools install` fetches the server and Chromium. Off when absent.
    #[serde(default)]
    pub enabled: bool,
    /// Whether the browser opens a window the maintainer can watch. Headless
    /// when absent.
    #[serde(default)]
    pub headed: bool,
    /// The hosts the browser may reach: a host, or `*.` and a parent for its
    /// subdomains. Every other host, and every IP address, fails to resolve.
    /// A port, an IP address and `localhost` are refused, since Chromium's
    /// resolver cannot hold a host to one port. The project's dev server is
    /// reached on its own address, which is not listed here. Empty when
    /// absent, so the browser opens nothing.
    #[serde(default, deserialize_with = "hosts")]
    pub domains: Vec<NonBlank>,
    /// The name of the launch configuration in `.claude/launch.json` on `main`
    /// that kelpie starts as the project's dev server for each worker turn
    /// that has the browser. The first one when absent. Nothing starts when
    /// `main` has no launch file.
    #[serde(default)]
    pub configuration: Option<NonBlank>,
    /// The paths kelpie opens on the dev server to prove a web bump
    /// (`dependencies.web`) in the browser: each must load, no request to the
    /// dev server may answer 5xx, and the console may log no error. Each starts
    /// with `/`. `["/"]` when absent.
    #[serde(default = "root", deserialize_with = "paths")]
    pub routes: Vec<String>,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            enabled: false,
            headed: false,
            domains: Vec::new(),
            configuration: None,
            routes: root(),
        }
    }
}

fn root() -> Vec<String> {
    vec!["/".to_owned()]
}

// Reads `routes`, which must be paths on the dev server: a list with at least one, each
// starting with `/`.
fn paths<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let routes = Vec::<String>::deserialize(deserializer)?;
    if routes.is_empty() {
        return Err(D::Error::custom(
            "`routes` is empty, so a web bump would open nothing: list a path, or leave it out for `/`",
        ));
    }
    for route in &routes {
        if !route.starts_with('/') {
            return Err(D::Error::custom(format!(
                "`{route}` is refused: a route is a path on the dev server and starts with `/`"
            )));
        }
    }
    Ok(routes)
}

// Reads `domains` as `worker.allowed_domains` is, then refuses what the browser's
// resolver rules cannot hold to: a port, an IP address, or a name for this machine.
fn hosts<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<NonBlank>, D::Error> {
    let entries = domains::allowed_domains(deserializer)?;
    for entry in &entries {
        let entry = entry.as_str();
        let refuse = |why: &str| D::Error::custom(format!("`{entry}` is refused: {why}"));
        if entry.starts_with('[') {
            return Err(refuse(
                "the browser reaches hosts by name, never an IP address",
            ));
        }
        if entry.contains(':') {
            return Err(refuse(
                "a port cannot be enforced on every request the browser makes, so list the host alone",
            ));
        }
        let host = entry.strip_prefix("*.").unwrap_or(entry);
        let host = host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase();
        let last = host.rsplit('.').next().unwrap_or(&host);
        if last.starts_with(|c: char| c.is_ascii_digit()) {
            return Err(refuse(
                "the browser reaches hosts by name, never an IP address",
            ));
        }
        if host == "localhost" || host.ends_with(".localhost") {
            return Err(refuse(
                "the browser reaches the project's dev server on its own address, and no other local host",
            ));
        }
    }
    Ok(entries)
}

impl Browser {
    /// Whether the machine this runs on can give workers a browser, or the
    /// table is off
    ///
    /// # Errors
    ///
    /// The reason, when the table is on and the machine is not a Mac.
    pub fn supported(&self) -> Result<(), String> {
        self.supported_on(cfg!(target_os = "macos"))
    }

    fn supported_on(&self, macos: bool) -> Result<(), String> {
        match (self.enabled, macos) {
            (true, false) => Err(
                "`browser.enabled` is on, but a worker's browser runs outside its sandbox on \
                 macOS only: turn it off, since Linux waits on shep-pm/shep-kelpie#135"
                    .to_owned(),
            ),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_enabled_browser_is_refused_off_macos_and_named_for_the_linux_issue() {
        let on = Browser {
            enabled: true,
            ..Browser::default()
        };
        assert_eq!(on.supported_on(true), Ok(()));
        let why = on.supported_on(false).unwrap_err();
        assert!(
            why.contains("browser.enabled") && why.contains("#135"),
            "{why}"
        );
        assert_eq!(Browser::default().supported_on(false), Ok(()));
    }

    #[test]
    fn the_browser_is_off_headless_and_reaches_nothing_unless_the_project_says() {
        let none: Browser = serde_json::from_value(json!({})).unwrap();
        assert_eq!(none, Browser::default());
        assert!(!none.enabled && !none.headed && none.domains.is_empty());
        let on: Browser = serde_json::from_value(json!({
            "enabled": true,
            "headed": true,
            "domains": ["app.example.com", "*.example.com"],
        }))
        .unwrap();
        assert!(on.enabled && on.headed);
        let listed: Vec<&str> = on.domains.iter().map(NonBlank::as_str).collect();
        assert_eq!(listed, ["app.example.com", "*.example.com"]);
    }

    #[test]
    fn the_launch_configuration_is_named_or_left_to_the_first() {
        let none: Browser = serde_json::from_value(json!({})).unwrap();
        assert_eq!(none.configuration, None);
        let named: Browser = serde_json::from_value(json!({ "configuration": "web" })).unwrap();
        assert_eq!(
            named.configuration.as_ref().map(NonBlank::as_str),
            Some("web")
        );
        assert!(serde_json::from_value::<Browser>(json!({ "configuration": " " })).is_err());
    }

    #[test]
    fn routes_default_to_the_root_and_each_must_start_with_a_slash() {
        let none: Browser = serde_json::from_value(json!({})).unwrap();
        assert_eq!(none.routes, ["/"]);
        let listed: Browser =
            serde_json::from_value(json!({ "routes": ["/", "/about", "/map?layer=1"] })).unwrap();
        assert_eq!(listed.routes, ["/", "/about", "/map?layer=1"]);
        for bad in ["about", "", "http://localhost:5173/", "?q=1", " /"] {
            let error = serde_json::from_value::<Browser>(json!({ "routes": ["/", bad] }));
            let why = error.unwrap_err().to_string();
            assert!(why.contains(&format!("`{bad}`")), "{bad:?}: {why}");
        }
        let empty = serde_json::from_value::<Browser>(json!({ "routes": [] }));
        assert!(empty.unwrap_err().to_string().contains("routes"));
    }

    #[test]
    fn a_key_the_table_does_not_have_is_refused_by_name() {
        let error = serde_json::from_value::<Browser>(json!({ "enable": true })).unwrap_err();
        assert!(error.to_string().contains("enable"), "{error}");
    }

    #[test]
    fn a_domain_the_sandbox_would_refuse_is_refused() {
        for bad in ["https://example.com", "a.*.com", " ", "example.com/path"] {
            let error = serde_json::from_value::<Browser>(json!({ "domains": [bad] }));
            assert!(error.is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_port_an_ip_address_and_localhost_are_refused_by_name() {
        for bad in [
            "app.example.com:8443",
            "*.example.com:443",
            "127.0.0.1",
            "10.0.0.5:3000",
            "127.1",
            "[::1]",
            "[::1]:3000",
            "localhost",
            "localhost:5173",
            "LOCALHOST",
            "app.localhost",
            "*.localhost",
            "localhost.",
        ] {
            let error = serde_json::from_value::<Browser>(json!({ "domains": [bad] }));
            let why = error.unwrap_err().to_string();
            assert!(why.contains(&format!("`{bad}`")), "{bad}: {why}");
        }
    }
}
