//! `[app.dogs.kelpie.tickets]`: where the project's tickets live

use schemars::JsonSchema;
use serde::Deserialize;

/// Where a project's tickets live
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TicketSource {
    /// GitHub issues in the project's repo
    #[default]
    Github,
    /// Markdown files in `tickets/` in the project's folder of kelpie's home
    Local,
}

impl TicketSource {
    /// The word the setting holds
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Local => "local",
        }
    }
}

/// Which source the project's tickets come from
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Tickets")]
pub struct Tickets {
    /// `github`, the project's issues, or `local`, markdown files that
    /// `shep kelpie ticket` files and reads, in `tickets/` in the project's
    /// folder of kelpie's home. A local ticket's id is `k` and a counter, its
    /// pull request carries no `Resolves` line, and kelpie closes it when the
    /// pull request merges. `github` when absent.
    #[serde(default)]
    pub source: TicketSource,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_with_no_source_takes_its_tickets_from_github() {
        let none: Tickets = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(none.source, TicketSource::Github);
        assert_eq!(Tickets::default().source, TicketSource::Github);
    }

    #[test]
    fn a_project_can_name_its_source() {
        for (word, want) in [
            ("github", TicketSource::Github),
            ("local", TicketSource::Local),
        ] {
            let named: Tickets =
                serde_json::from_value(serde_json::json!({ "source": word })).unwrap();
            assert_eq!(named.source, want);
            assert_eq!(want.as_str(), word);
        }
    }

    #[test]
    fn a_source_or_key_the_table_does_not_have_is_refused() {
        let clickup = serde_json::json!({ "source": "clickup" });
        assert!(serde_json::from_value::<Tickets>(clickup).is_err());
        let wrong = serde_json::json!({ "sources": "local" });
        assert!(serde_json::from_value::<Tickets>(wrong).is_err());
    }
}
