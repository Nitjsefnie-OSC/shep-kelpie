//! The id of the ticket a work item is for
//!
//! A GitHub ticket's id is its issue number, written `340`. A local ticket's
//! is `k` and a counter, written `k12`. A project on GitHub keeps every
//! branch, folder and trigger name it has, since an id displays as it parses.

use core::fmt;
use core::num::NonZeroU64;
use core::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::state::{Finished, Ruling};
use crate::work_item::WorkItem;

/// The id of a ticket
///
/// Ordered by source, GitHub's before the local ones, then by number, so the
/// board's age tie-breaker holds within a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TicketId {
    /// A GitHub issue, by its number
    Github(NonZeroU64),
    /// A local ticket, by its counter
    Local(NonZeroU64),
}

/// Why text is not a [`TicketId`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketIdError {
    /// It is not digits, or `k` and digits, or it has a sign, a space or a leading zero
    Malformed,
    /// Its number is zero, which no source numbers a ticket
    Zero,
    /// Its number is more than a `u64` holds
    TooBig,
}

impl fmt::Display for TicketIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "a ticket id is digits, or `k` and digits, with no leading zero",
            Self::Zero => "a ticket id is not zero",
            Self::TooBig => "a ticket id is more than a u64 holds",
        })
    }
}

impl core::error::Error for TicketIdError {}

impl TicketId {
    /// The id of a GitHub issue, or None for zero
    pub fn from_issue(number: u64) -> Option<Self> {
        NonZeroU64::new(number).map(Self::Github)
    }

    /// The issue number of a GitHub ticket, for code that only GitHub's tickets
    /// reach, such as triage, adoption and `Resolves #N`. None for a local one.
    pub fn github(&self) -> Option<u64> {
        match self {
            Self::Github(number) => Some(number.get()),
            Self::Local(_) => None,
        }
    }
}

impl fmt::Display for TicketId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Github(number) => write!(f, "{number}"),
            Self::Local(counter) => write!(f, "k{counter}"),
        }
    }
}

impl FromStr for TicketId {
    type Err = TicketIdError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (digits, local) = match text.strip_prefix('k') {
            Some(digits) => (digits, true),
            None => (text, false),
        };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(TicketIdError::Malformed);
        }
        if digits.len() > 1 && digits.starts_with('0') {
            return Err(TicketIdError::Malformed);
        }
        let number: u64 = digits.parse().map_err(|_| TicketIdError::TooBig)?;
        let number = NonZeroU64::new(number).ok_or(TicketIdError::Zero)?;
        Ok(if local {
            Self::Local(number)
        } else {
            Self::Github(number)
        })
    }
}

impl Serialize for TicketId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TicketId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct IdVisitor;

        impl Visitor<'_> for IdVisitor {
            type Value = TicketId;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an issue number, or a ticket id such as `340` or `k12`")
            }

            fn visit_u64<E: de::Error>(self, number: u64) -> Result<TicketId, E> {
                TicketId::from_issue(number)
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Unsigned(number), &self))
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<TicketId, E> {
                text.parse()
                    .map_err(|_| E::invalid_value(de::Unexpected::Str(text), &self))
            }
        }

        deserializer.deserialize_any(IdVisitor)
    }
}

impl WorkItem {
    /// The ticket it is for, from its issue number. None only for issue zero,
    /// which no forge numbers.
    pub fn ticket(&self) -> Option<TicketId> {
        TicketId::from_issue(self.issue)
    }
}

impl Ruling {
    /// The ticket of the work item it parks, once that is known
    pub fn ticket(&self) -> Option<TicketId> {
        self.issue.and_then(TicketId::from_issue)
    }
}

impl Finished {
    /// The ticket it resolved, from its issue number. None only for issue zero.
    pub fn ticket(&self) -> Option<TicketId> {
        TicketId::from_issue(self.issue)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::TicketId;
    use crate::ports::{SessionId, Timestamp};
    use crate::state::{Finished, Ruling, RulingKind, Stuck};
    use crate::work_item::{Seconds, WorkItem};

    fn id(text: &str) -> TicketId {
        text.parse().unwrap()
    }

    // text, github(), and what it displays as
    #[test]
    fn an_id_parses_and_displays_as_written() {
        let cases = [
            ("1", Some(1)),
            ("340", Some(340)),
            ("18446744073709551615", Some(u64::MAX)),
            ("k1", None),
            ("k12", None),
        ];
        for (text, github) in cases {
            let ticket = id(text);
            assert_eq!(ticket.to_string(), text, "{text}");
            assert_eq!(ticket.github(), github, "{text}");
        }
    }

    #[test]
    fn what_is_not_an_id_is_refused() {
        for text in [
            "",
            "#7",
            "0",
            "k",
            "k0",
            "K12",
            "-7",
            "+7",
            "007",
            "k012",
            " 7",
            "7 ",
            "7a",
            "kk1",
            "k-1",
            "k1.5",
            "18446744073709551616",
            "k18446744073709551616",
        ] {
            assert!(text.parse::<TicketId>().is_err(), "{text:?} parsed");
        }
    }

    #[test]
    fn ids_order_by_source_then_number() {
        let mut ids: Vec<TicketId> = ["k2", "10", "k10", "9", "2", "k1"].map(id).into();
        ids.sort();
        let sorted: Vec<String> = ids.iter().map(ToString::to_string).collect();
        assert_eq!(sorted, ["2", "9", "10", "k1", "k2", "k10"]);
    }

    #[test]
    fn an_id_is_written_as_a_string() {
        assert_eq!(serde_json::to_value(id("340")).unwrap(), json!("340"));
        assert_eq!(serde_json::to_value(id("k12")).unwrap(), json!("k12"));
    }

    #[test]
    fn an_id_is_read_from_a_number_or_a_string() {
        let cases = [
            (json!(340), "340"),
            (json!("340"), "340"),
            (json!("k12"), "k12"),
        ];
        for (value, shown) in cases {
            let ticket: TicketId = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(ticket.to_string(), shown, "{value}");
        }
    }

    #[test]
    fn what_is_not_an_id_is_not_read() {
        for value in [
            json!(0),
            json!("0"),
            json!(-7),
            json!(1.5),
            json!("#7"),
            json!("k"),
            json!("k0"),
            json!("K12"),
            json!(12),
            json!(null),
            json!(true),
            json!([7]),
        ] {
            let read = serde_json::from_value::<TicketId>(value.clone());
            // 12 is the one value here that is an id, kept so the loop shows
            // the list is not refusing everything
            assert_eq!(read.is_ok(), value == json!(12), "{value}");
        }
    }

    #[test]
    fn a_work_item_is_for_the_ticket_of_its_issue() {
        let item = WorkItem::new(
            340,
            "A title",
            "kelpie/340",
            PathBuf::from("wt"),
            PathBuf::from("build"),
            "sonnet-high".to_owned().try_into().unwrap(),
            SessionId("s".to_owned()),
        );
        assert_eq!(item.ticket(), Some(id("340")));
        assert_eq!(WorkItem { issue: 0, ..item }.ticket(), None);
    }

    #[test]
    fn a_ruling_is_for_the_ticket_of_its_issue_if_it_has_one() {
        let ruling = Ruling::new(5, "Which?", RulingKind::from(Stuck::Closed));
        assert_eq!(ruling.ticket(), None);
        let ruling = Ruling {
            issue: Some(340),
            ..ruling
        };
        assert_eq!(ruling.ticket(), Some(id("340")));
    }

    #[test]
    fn a_finished_work_item_is_for_the_ticket_of_its_issue() {
        let record = Finished {
            issue: 340,
            title: "A title".to_owned(),
            pull_request: None,
            merged: false,
            closed: true,
            at: Timestamp(1),
            wall: 0,
            seconds: Seconds::default(),
            spend: None,
        };
        assert_eq!(record.ticket(), Some(id("340")));
    }
}
