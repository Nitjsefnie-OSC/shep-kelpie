//! What the maintainer's answer says to the worker, as its next turn

use crate::runner::gate::short;
use crate::runner::turn::KEEP_BODY_TRUE;

// Asked only when there is a pull request whose body could be stale.
fn keep_body_true(number: Option<u64>) -> String {
    number.map_or_else(String::new, |_| format!(" {KEEP_BODY_TRUE}"))
}

pub(super) fn note_prompt(number: Option<u64>, note: &str) -> String {
    let about = number.map_or_else(
        || "your work item".to_owned(),
        |n| format!("pull request #{n}"),
    );
    let mut prompt = format!("The maintainer answered no on {about}, with this note:\n\n{note}\n");
    if number.is_some() {
        prompt.push_str(&format!("\n{KEEP_BODY_TRUE}\n"));
    }
    prompt
}

pub(super) fn rework_prompt(number: Option<u64>, note: &str) -> String {
    let about = number.map_or_else(
        || "your work item".to_owned(),
        |n| format!("pull request #{n}"),
    );
    format!(
        "The maintainer asked for a rework of {about}, with this note:\n\n{note}\n\n\
         Once you push it, the whole review reads your change again.{}\n",
        keep_body_true(number)
    )
}

pub(super) fn declined_prompt(number: Option<u64>, head: &str, note: &str) -> String {
    let about = number.map_or_else(
        || "your branch".to_owned(),
        |n| format!("pull request #{n}"),
    );
    format!(
        "Someone other than you pushed commit {} to {about}, and the maintainer \
         declined it, with this note:\n\n{note}\n\nYour worktree is now at that commit. \
         Revert or change it with a new commit on top, and push with \
         `git push origin HEAD`. Do not force-push.{}\n",
        short(head),
        keep_body_true(number)
    )
}

pub(super) fn answer_prompt(text: &str) -> String {
    format!("The maintainer answered your question:\n\n{text}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_pull_request_yet_no_answer_asks_for_a_body() {
        let prompts = [
            note_prompt(None, "rename it"),
            rework_prompt(None, "rename it"),
            declined_prompt(None, "0123456789", "rename it"),
        ];
        for prompt in prompts {
            assert!(!prompt.contains("body"), "{prompt}");
        }
    }
}
