//! What the maintainer's answer says to the worker, as its next turn

use crate::runner::gate::short;
use crate::runner::turn::KEEP_BODY_TRUE;
use crate::skills::split_command;

// Asked only when there is a pull request whose body could be stale.
fn keep_body_true(number: Option<u64>) -> String {
    number.map_or_else(String::new, |_| format!(" {KEEP_BODY_TRUE}"))
}

// A turn that failed before the harness started never saw its prompt. A
// command only runs from the very start of a prompt, so the line goes after it.
pub(super) fn failed_again(prompt: String) -> String {
    let line = "Your last turn failed before it finished. Kelpie sends its prompt again.";
    match split_command(&prompt) {
        // A retry that fails again is kept with its line already on it.
        (_, rest) if rest.starts_with(line) => prompt,
        (Some(command), rest) => format!("{command} {line}\n\n{rest}"),
        (None, rest) => format!("{line}\n\n{rest}"),
    }
}

// A no on an over-budget ruling sends its note and then the turn that was due,
// which would otherwise be lost. A command only runs from the very start of a
// prompt, so the note goes after it.
pub(super) fn noted_prompt(note: &str, owed: &str) -> String {
    let lead = format!("{note}\nThe turn that was due is still yours, and follows:\n\n");
    match split_command(owed) {
        (Some(command), rest) => format!("{command} {lead}{rest}"),
        (None, rest) => format!("{lead}{rest}"),
    }
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
