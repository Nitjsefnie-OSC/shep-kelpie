//! What kelpie has made of the comments people wrote on a pull request

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How many rounds of comments one work item may send the worker before the
/// next is the maintainer's to rule on
pub const TALK_ROUNDS: u32 = 3;

/// Which comments on the pull request kelpie has read, and what they cost
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Talked {
    /// Whether the pull request's comments have been read at all
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read: bool,
    /// The forge's ids of the conversation comments handled: sent to the
    /// worker, answered as a ruling, or ignored
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<u64>,
    /// For each review thread, the id of the latest comment handled
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub threads: BTreeMap<String, u64>,
    /// The rounds of comments sent to the worker so far
    #[serde(default, skip_serializing_if = "super::is_zero")]
    pub rounds: u32,
    /// The head on `origin` when a round was sent, while the threads it sent
    /// are still to be replied to
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
}

impl Talked {
    /// Whether nothing has been read or sent
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Whether the rounds sent have reached [`TALK_ROUNDS`]
    pub fn capped(&self) -> bool {
        self.rounds >= TALK_ROUNDS
    }

    /// Notes conversation comment `id` as handled
    pub fn hear(&mut self, id: u64) {
        if !self.comments.contains(&id) {
            self.comments.push(id);
        }
    }

    /// Notes that thread `thread` is handled up to its comment `id`
    pub fn hear_thread(&mut self, thread: &str, id: u64) {
        let seen = self.threads.entry(thread.to_owned()).or_default();
        *seen = (*seen).max(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_is_handled_up_to_its_latest_comment_and_never_back() {
        let mut talked = Talked::default();
        talked.hear_thread("PRRT_a", 9);
        talked.hear_thread("PRRT_a", 4);
        assert_eq!(talked.threads["PRRT_a"], 9);
    }

    #[test]
    fn a_comment_is_handled_once() {
        let mut talked = Talked::default();
        talked.hear(5);
        talked.hear(5);
        assert_eq!(talked.comments, [5]);
    }

    #[test]
    fn nothing_read_is_left_out_of_the_state_file() {
        assert_eq!(serde_json::to_string(&Talked::default()).unwrap(), "{}");
    }

    #[test]
    fn the_cap_is_reached_after_three_rounds() {
        let rounds = |rounds| Talked {
            rounds,
            ..Talked::default()
        };
        assert!(!rounds(2).capped());
        assert!(rounds(3).capped());
    }
}
