//! What kelpie believes a pull request carries, and what changed behind its back

use serde::{Deserialize, Serialize};

/// The labels, ready state and head kelpie believes a pull request carries
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Known {
    /// Its labels' names
    pub labels: Vec<String>,
    /// Whether it is marked ready for review (not a draft)
    pub ready: bool,
    /// Its head as the worker's turn or kelpie's own push left it, once
    /// kelpie has read one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
}

/// Whether `labels`, `ready` and `head`, as seen on the forge, differ from
/// `known`, and if so what changed, named plainly enough to answer from a phone
///
/// A `known` with no head makes no claim about it.
pub fn foreign_change(
    known: &Known,
    labels: &[String],
    ready: bool,
    head: &str,
) -> Option<(Known, String)> {
    let added: Vec<&String> = labels
        .iter()
        .filter(|l| !known.labels.contains(l))
        .collect();
    let removed: Vec<&String> = known
        .labels
        .iter()
        .filter(|l| !labels.contains(l))
        .collect();
    let mut parts = Vec::new();
    if !added.is_empty() {
        parts.push(format!(
            "the `{}` {} added",
            joined(&added),
            label_or_labels(added.len())
        ));
    }
    if !removed.is_empty() {
        parts.push(format!(
            "the `{}` {} removed",
            joined(&removed),
            label_or_labels(removed.len())
        ));
    }
    if ready && !known.ready {
        parts.push("it was marked ready for review".to_owned());
    } else if !ready && known.ready {
        parts.push("it was marked a draft again".to_owned());
    }
    if known.head.as_deref().is_some_and(|h| h != head) {
        let short = head.get(..7).unwrap_or(head);
        parts.push(format!(
            "its head moved to {short}, a commit the worker did not push"
        ));
    }
    if parts.is_empty() {
        return None;
    }
    let seen = Known {
        labels: labels.to_vec(),
        ready,
        head: Some(head.to_owned()),
    };
    Some((seen, parts.join("; ")))
}

fn label_or_labels(n: usize) -> &'static str {
    if n == 1 { "label was" } else { "labels were" }
}

fn joined(labels: &[&String]) -> String {
    labels
        .iter()
        .map(|l| l.as_str())
        .collect::<Vec<_>>()
        .join("`, `")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_changed_is_no_foreign_change() {
        let known = Known {
            labels: vec!["bug".into()],
            ready: true,
            head: None,
        };
        assert_eq!(
            foreign_change(&known, &["bug".to_owned()], true, "c0ffee"),
            None
        );
    }

    #[test]
    fn one_label_added_is_named_in_the_singular() {
        let known = Known::default();
        let (seen, text) = foreign_change(&known, &["bug".to_owned()], false, "c0ffee").unwrap();
        assert_eq!(text, "the `bug` label was added");
        assert_eq!(
            seen,
            Known {
                labels: vec!["bug".into()],
                ready: false,
                head: Some("c0ffee".into()),
            }
        );
    }

    #[test]
    fn two_labels_added_are_named_in_the_plural() {
        let known = Known::default();
        let labels = ["urgent".to_owned(), "bug".to_owned()];
        let (_, text) = foreign_change(&known, &labels, false, "c0ffee").unwrap();
        assert_eq!(text, "the `urgent`, `bug` labels were added");
    }

    #[test]
    fn labels_removed_are_named_with_the_same_singular_and_plural_rule() {
        let known = Known {
            labels: vec!["bug".into()],
            ready: false,
            head: None,
        };
        let (_, text) = foreign_change(&known, &[], false, "c0ffee").unwrap();
        assert_eq!(text, "the `bug` label was removed");

        let known = Known {
            labels: vec!["urgent".into(), "bug".into()],
            ready: false,
            head: None,
        };
        let (_, text) = foreign_change(&known, &[], false, "c0ffee").unwrap();
        assert_eq!(text, "the `urgent`, `bug` labels were removed");
    }

    #[test]
    fn marking_ready_or_a_draft_again_is_named() {
        let known = Known::default();
        let (seen, text) = foreign_change(&known, &[], true, "c0ffee").unwrap();
        assert_eq!(text, "it was marked ready for review");
        assert!(seen.ready);

        let known = Known {
            labels: vec![],
            ready: true,
            head: None,
        };
        let (seen, text) = foreign_change(&known, &[], false, "c0ffee").unwrap();
        assert_eq!(text, "it was marked a draft again");
        assert!(!seen.ready);
    }

    #[test]
    fn every_kind_of_change_at_once_is_joined_with_semicolons() {
        let known = Known {
            labels: vec!["bug".into()],
            ready: false,
            head: Some("a11ce".into()),
        };
        let (_, text) = foreign_change(&known, &["urgent".to_owned()], true, "c0ffee").unwrap();
        assert_eq!(
            text,
            "the `urgent` label was added; the `bug` label was removed; \
             it was marked ready for review; \
             its head moved to c0ffee, a commit the worker did not push"
        );
    }

    #[test]
    fn a_head_the_worker_did_not_push_is_named_by_its_short_hash() {
        let known = Known {
            head: Some("a11ce".into()),
            ..Known::default()
        };
        let head = "4887ecf0123456789";
        let (seen, text) = foreign_change(&known, &[], false, head).unwrap();
        assert_eq!(
            text,
            "its head moved to 4887ecf, a commit the worker did not push"
        );
        assert_eq!(seen.head.as_deref(), Some(head));
        assert_eq!(foreign_change(&seen, &[], false, head), None);
    }

    #[test]
    fn a_known_with_no_head_makes_no_claim_about_it() {
        assert_eq!(
            foreign_change(&Known::default(), &[], false, "c0ffee"),
            None
        );
    }
}
