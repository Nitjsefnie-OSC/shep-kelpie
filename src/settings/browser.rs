//! `[app.dogs.kelpie.browser]`: whether a worker drives a browser to see the UI it builds

use schemars::JsonSchema;
use serde::Deserialize;

use super::{NonBlank, domains};

/// Whether the project's workers are given a browser outside their sandbox
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
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
    /// The hosts the browser may reach, in the form of `worker.allowed_domains`:
    /// a host, or `*.` and a parent for its subdomains, with an optional
    /// `:port`. Every other host, and every IP address, fails to resolve.
    /// Empty when absent, so the browser opens nothing.
    #[serde(default, deserialize_with = "domains::allowed_domains")]
    pub domains: Vec<NonBlank>,
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
            "domains": ["app.example.com:5173", "*.example.com"],
        }))
        .unwrap();
        assert!(on.enabled && on.headed);
        let listed: Vec<&str> = on.domains.iter().map(NonBlank::as_str).collect();
        assert_eq!(listed, ["app.example.com:5173", "*.example.com"]);
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
}
