//! Telling a contributor what kelpie does to their branch once it is kelpie's
//!
//! After a release the contributor can still be watching. When red CI sends
//! the worker a fix turn on their branch, the representative posts one comment
//! saying kelpie is on it, once for each fix turn. The failed checks' names go
//! to the call as facts, fenced as data since a workflow file names them. The
//! comment is a courtesy: a call or a post that fails is told in the log and
//! not tried again, and the fix turn never waits on it. It never promises a
//! merge, which only the maintainer's ruling decides.
//!
//! A push of the contributor's after the release is no case of its own: it
//! moves the head kelpie knew, and the gate raises the foreign-change ruling
//! as it does for any adopted pull request.

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
