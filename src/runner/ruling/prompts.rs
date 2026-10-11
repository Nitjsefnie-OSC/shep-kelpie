//! What the maintainer's answer says to the worker, as its next turn

use crate::runner::gate::short;

pub(super) fn note_prompt(number: Option<u64>, note: &str) -> String {
    let about = number.map_or_else(
        || "your work item".to_owned(),
        |n| format!("pull request #{n}"),
    );
    format!("The maintainer answered no on {about}, with this note:\n\n{note}\n")
}

pub(super) fn rework_prompt(number: Option<u64>, note: &str) -> String {
    let about = number.map_or_else(
        || "your work item".to_owned(),
        |n| format!("pull request #{n}"),
    );
    format!(
        "The maintainer asked for a rework of {about}, with this note:\n\n{note}\n\n\
         Once you push it, the whole review reads your change again.\n"
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
         `git push origin HEAD`. Do not force-push.\n",
        short(head)
    )
}

pub(super) fn answer_prompt(text: &str) -> String {
    format!("The maintainer answered your question:\n\n{text}\n")
}
