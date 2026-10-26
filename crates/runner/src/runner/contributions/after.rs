//! Telling a contributor what kelpie does to their branch once it is kelpie's
//!
//! When red CI sends the worker a fix turn, the representative posts one
//! comment saying kelpie is on it, once per fix turn. The failed checks' names
//! go to the call as facts, fenced as data. A failed call or post is logged and
//! not retried, and the fix turn never waits on it. It never promises a merge.
//!
//! A contributor's push after the release moves the head, and the gate raises
//! the foreign-change ruling as for any adopted pull request.

use super::super::Runner;
use super::super::representative::Post;
use super::super::untrusted::fenced;
use super::post::short;

/// What kind of post the representative is asked for
const FIXING: &str = "Tell the contributor that CI failed on their pull request and that kelpie \
    is fixing it on their branch. Friendly and short. Name the failed checks. Never promise or \
    hint at a merge: only the maintainer's ruling decides one.";

impl Runner {
    /// Posts, as the representative, that kelpie is fixing red CI on the
    /// contributor's pull request, when the current work item adopted one
    ///
    /// Called once a fix turn has started. Nothing here fails the caller.
    pub(in crate::runner) fn tell_of_fix(&mut self, number: u64, head: &str, checks: &[String]) {
        let Some(contributor) = self.current().and_then(|item| item.contributor.clone()) else {
            return;
        };
        let facts = format!(
            "Pull request #{number} by @{contributor}, at {}.\n\
             CI failed on it, and kelpie's worker is fixing it: it will commit a fix and push it \
             to the contributor's branch. The contributor's own commits stay.\n\
             {}\n",
            short(head),
            fenced(
                "The failed checks' names, one to a line,",
                &checks.join("\n")
            )
        );
        let post = Post {
            pull_request: number,
            kind: FIXING,
            facts: &facts,
            earlier: &[],
            contributor: &contributor,
            said: "",
        };
        let posted = self
            .represent(&post)
            .map_err(|e| e.to_string())
            .and_then(|words| {
                (self.ports.voice)
                    .post(&self.remote, number, &words)
                    .map_err(|e| e.to_string())
            });
        if let Err(why) = posted {
            self.notes.push(format!(
                "contribution #{number}: cannot tell the contributor kelpie is fixing CI: {why}"
            ));
        }
    }
}
