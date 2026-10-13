//! The ruling for a reviewer's test that still fails after two fixes

use super::prompts::note_prompt;
use crate::runner::gate::short;
use crate::state::{FailingTest, Fix};
use crate::work_item::{Phase, Review, ReviewStage};

/// What a reader of the pull request is told: a test fails, and nothing of it
pub(super) const PUBLIC_LINE: &str = "A reviewer's test still fails after two fixes.";

// The question's body: the review, the head and every test with its output tail.
pub(super) fn question(
    about: &str,
    (yes, no): (&str, &str),
    fix: &Fix,
    head: &str,
    tests: &[FailingTest],
) -> String {
    let Fix::Review(review) = fix;
    let failing: Vec<String> = tests
        .iter()
        .map(|t| format!("{}:\n{}", t.test, t.output.trim_end()))
        .collect();
    format!(
        "A test a reviewer wrote for round {} of the review on {about} still fails at {} after the \
         worker's two fixes:\n\n{}\n\n{yes} sends the worker one more fix turn on the same \
         tests, and {no} ends the check, sends the worker your note and lets the review go on",
        review.round,
        short(head),
        failing.join("\n\n")
    )
}

// A yes: the worker's turn ends under the same round, which runs the tests again.
pub(super) fn yes(fix: Fix, prompt: String) -> (String, Phase) {
    let Fix::Review(review) = fix;
    (prompt, Phase::Review(review))
}

// A no: the worker takes the note under a fix stage that has nothing to check,
// so the turn ends in the pass going on to its next reviewer.
pub(super) fn no(number: Option<u64>, fix: Fix, note: &str) -> (String, Phase) {
    let Fix::Review(review) = fix;
    let review = Review {
        stage: ReviewStage::Fixing {
            head: None,
            sent: Vec::new(),
            deferred_before: Vec::new(),
        },
        ..review
    };
    (note_prompt(number, note), Phase::Review(review))
}
