use crate::adapters::gh::review_bot::{parse_comments, parse_reactions};
use crate::codex::*;
use crate::ports::{Severity, Timestamp};
use crate::review_bot::{Activity, Profile, Reading, Thread};
use crate::review_bot::{Comment, Reaction, Review};

// Recorded from shep-pm/shep-kelpie#234 with the gh adapter's own call:
// the maintainer's summon at 04:39:19Z drew "create an environment" at
// 04:39:27Z, and its review of 8eecd21 at 04:43:07Z found nothing.
const COMMENTS_234: &str = include_str!("../../../fixtures/codex-comments-234.jsonl");
const REACTIONS_234: &str = include_str!("../../../fixtures/codex-reactions-234.json");
const HEAD_234: &str = "8eecd218357d9d3a017baeff71c374f6865466e1";

fn iso(text: &str) -> Timestamp {
    let at: jiff::Timestamp = text.parse().unwrap();
    Timestamp(at.as_second().try_into().unwrap())
}

fn activity(comments: &str) -> Activity {
    Activity {
        comments: parse_comments(comments.as_bytes()).unwrap(),
        ..Activity::default()
    }
}

fn thread(body: &str) -> Thread {
    Thread {
        id: "t".into(),
        resolved: false,
        outdated: false,
        path: "src/a.rs".into(),
        line: Some(12),
        body: body.into(),
    }
}

fn comment(body: &str, at: u64) -> Comment {
    Comment {
        body: body.into(),
        at: Timestamp(at),
    }
}

#[test]
fn its_first_comment_is_the_sign_it_heard_and_no_refusal() {
    let summoned = iso("2026-10-01T04:39:19Z");
    let answered = activity(COMMENTS_234.lines().next().unwrap());
    assert!(answered.comments[0].body.contains("create an environment"));
    assert!(Codex.heard(&answered, HEAD_234, summoned));
    assert_eq!(
        Codex.read(&answered, HEAD_234, summoned),
        Reading::Processing
    );
    let later = iso("2026-10-01T05:00:00Z");
    assert!(!Codex.heard(&answered, HEAD_234, later), "a later summon");
    assert_eq!(Codex.read(&answered, HEAD_234, later), Reading::Silent);
}

#[test]
fn its_comment_naming_the_commit_covers_it_by_the_ten_characters_it_gives() {
    let seen = activity(COMMENTS_234);
    let summoned = iso("2026-10-01T04:39:19Z");
    assert_eq!(Codex.read(&seen, HEAD_234, summoned), Reading::Reviewed);
    assert!(!Codex.covers(&seen, "8eecd21fffffff"));
    assert!(!Codex.covers(&seen, ""));
    assert_eq!(Codex.reviewed_besides(&seen, HEAD_234), 0);
    assert_eq!(Codex.reviewed_besides(&seen, "0ther"), 1);
    assert_eq!(Codex.quota(&seen), None);
}

#[test]
fn a_review_covers_the_commit_it_read() {
    let seen = Activity {
        reviews: vec![Review {
            commit: "abc123".into(),
            body: "### 💡 Codex Review\n\nHere are some automated review suggestions.".into(),
            at: Timestamp(200),
        }],
        ..Activity::default()
    };
    assert_eq!(
        Codex.read(&seen, "abc123", Timestamp(100)),
        Reading::Reviewed
    );
    assert!(!Codex.covers(&seen, "other"));
    assert_eq!(Codex.reviewed_besides(&seen, "other"), 1);
}

#[test]
fn a_reply_in_a_thread_is_no_review() {
    let seen = Activity {
        reviews: vec![Review {
            commit: "abc123".into(),
            body: "Thanks, fixed.".into(),
            at: Timestamp(200),
        }],
        ..Activity::default()
    };
    assert!(!Codex.covers(&seen, "abc123"));
}

#[test]
fn a_usage_limit_reply_is_a_refusal_with_no_opening() {
    let refusal = Activity {
        comments: vec![comment(
            "You have reached your Codex usage limits for code reviews. You can see \
             your limits in the [Codex usage dashboard](https://chatgpt.com/codex/settings/usage).",
            130,
        )],
        ..Activity::default()
    };
    assert_eq!(
        Codex.read(&refusal, "abc123", Timestamp(100)),
        Reading::Refused { opens: None }
    );
    assert!(Codex.heard(&refusal, "abc123", Timestamp(100)));
    let mut then_more = refusal.clone();
    then_more
        .comments
        .push(comment("To use Codex here, create an environment.", 140));
    assert_eq!(
        Codex.read(&then_more, "abc123", Timestamp(100)),
        Reading::Refused { opens: None },
        "a later comment does not hide it"
    );
}

const WENT_WRONG: &str = "Codex Review: Something went wrong. Try again later by \
                          commenting \"@codex review\".\n\nUnknown error";

#[test]
fn a_something_went_wrong_reply_is_read_as_a_failed_review() {
    let failed = Activity {
        comments: vec![comment(WENT_WRONG, 130)],
        ..Activity::default()
    };
    assert_eq!(
        Codex.read(&failed, "abc123", Timestamp(100)),
        Reading::Failed
    );
    let after_notice = Activity {
        comments: vec![
            comment("To use Codex here, create an environment.", 110),
            comment(WENT_WRONG, 130),
        ],
        ..Activity::default()
    };
    assert_eq!(
        Codex.read(&after_notice, "abc123", Timestamp(100)),
        Reading::Failed,
        "the notice before it does not hide it"
    );
}

#[test]
fn a_review_comment_naming_the_commit_is_never_read_as_a_failure() {
    let seen = Activity {
        comments: vec![comment(
            "Codex Review: Something went wrong in `retry.rs`.\n\n**Reviewed commit:** `abc123`",
            130,
        )],
        ..Activity::default()
    };
    assert_eq!(
        Codex.read(&seen, "abc123def", Timestamp(100)),
        Reading::Reviewed
    );
    assert_eq!(
        Codex.read(&seen, "other", Timestamp(100)),
        Reading::Processing,
        "a review of another commit is no failure either"
    );
}

#[test]
fn a_failure_before_the_summon_is_not_its_answer() {
    let failed = Activity {
        comments: vec![comment(WENT_WRONG, 95)],
        ..Activity::default()
    };
    assert_eq!(
        Codex.read(&failed, "abc123", Timestamp(100)),
        Reading::Processing,
        "heard within the clock's slack, but no answer to it"
    );
    assert_eq!(
        Codex.read(&failed, "abc123", Timestamp(40)),
        Reading::Failed
    );
}

#[test]
fn a_thread_reaches_the_worker_with_its_badge_and_title() {
    let first = Codex.finding(&thread(
        "**<sub><sub>![P1 Badge](https://img.shields.io/badge/P1-orange?style=flat)</sub></sub>  \
         Guard against empty input**\n\n`parse()` panics when `input` is empty.\n\n\
         Useful? React with 👍 / 👎.",
    ));
    assert_eq!(first.severity, Severity::High);
    assert_eq!((first.file.as_str(), first.line), ("src/a.rs", 12));
    assert_eq!(
        first.what,
        "Guard against empty input: `parse()` panics when `input` is empty."
    );
    assert_eq!(first.why, "Codex rates it P1.");
}

#[test]
fn a_p2_is_medium_a_p3_a_nit_and_a_thread_with_no_badge_is_medium() {
    let level = |badge: &str| {
        Codex
            .finding(&thread(&format!(
                "**<sub><sub>![{badge} Badge](https://img.shields.io/badge/x)</sub></sub>  T**\n\nWhy."
            )))
            .severity
    };
    assert_eq!(level("P2"), Severity::Medium);
    assert_eq!(level("P3"), Severity::Low);
    assert_eq!(level("P0"), Severity::High);
    let bare = Codex.finding(&thread("It drops the error."));
    assert_eq!(bare.severity, Severity::Medium);
    assert_eq!(
        (bare.what.as_str(), bare.why.as_str()),
        ("It drops the error.", "")
    );
}

#[test]
fn its_thumbs_up_recorded_on_the_pull_request_is_a_review_with_nothing_in_it() {
    let seen = Activity {
        reactions: parse_reactions(REACTIONS_234.as_bytes(), LOGIN.rest).unwrap(),
        ..Activity::default()
    };
    let summoned = iso("2026-10-01T04:39:19Z");
    assert!(Codex.heard(&seen, HEAD_234, summoned));
    assert_eq!(
        Codex.read(&seen, HEAD_234, summoned),
        Reading::Completed {
            at: iso("2026-10-01T04:43:06Z")
        }
    );
    let later = iso("2026-10-01T05:00:00Z");
    assert_eq!(Codex.read(&seen, HEAD_234, later), Reading::Silent);
}

#[test]
fn a_thumbs_up_does_not_hide_a_refusal_and_another_reaction_is_no_review() {
    let eyes = Reaction {
        content: "EYES".into(),
        at: Timestamp(120),
    };
    let seen = Activity {
        reactions: vec![eyes.clone()],
        ..Activity::default()
    };
    assert!(Codex.heard(&seen, "abc123", Timestamp(100)));
    assert_eq!(Codex.read(&seen, "abc123", Timestamp(100)), Reading::Silent);
    let refused = Activity {
        comments: vec![comment("You've hit your usage limit for reviews.", 130)],
        reactions: vec![Reaction {
            content: "THUMBS_UP".into(),
            at: Timestamp(140),
        }],
        ..Activity::default()
    };
    assert_eq!(
        Codex.read(&refused, "abc123", Timestamp(100)),
        Reading::Refused { opens: None }
    );
}

#[test]
fn a_refusal_in_other_words_is_still_one_and_the_review_footer_is_not() {
    assert!(refused("Usage limit reached for code reviews."));
    assert!(refused("You have exceeded your limit this week."));
    assert!(!refused(
        "Codex Review: Didn't find any major issues.\n\nCodex can also answer questions."
    ));
    assert!(!refused(
        "To use Codex here, create an environment for this repo."
    ));
}

#[test]
fn one_commit_read_by_a_review_and_by_a_comment_counts_once() {
    let seen = Activity {
        reviews: vec![Review {
            commit: "8eecd218357d9d3a017baeff71c374f6865466e1".into(),
            body: "### 💡 Codex Review".into(),
            at: Timestamp(100),
        }],
        comments: vec![comment("**Reviewed commit:** `8eecd21835`", 200)],
        ..Activity::default()
    };
    assert_eq!(Codex.reviewed_besides(&seen, "0ther"), 1);
    assert_eq!(Codex.reviewed_besides(&seen, HEAD_234), 0);
}

#[test]
fn a_thumbs_up_beside_a_review_of_another_commit_is_not_a_review_of_the_head() {
    let seen = Activity {
        comments: vec![comment(
            "Codex Review: Didn't find any major issues.\n\n**Reviewed commit:** `aaaaaaaaaa`",
            200,
        )],
        reactions: vec![Reaction {
            content: "THUMBS_UP".into(),
            at: Timestamp(201),
        }],
        ..Activity::default()
    };
    let head = "bbbbbbbbbbbbbbbbbbbb";
    assert_eq!(Codex.read(&seen, head, Timestamp(100)), Reading::Processing);
    assert_eq!(
        Codex.read(&seen, "aaaaaaaaaabbbb", Timestamp(100)),
        Reading::Reviewed
    );
}

#[test]
fn a_stale_clean_comment_or_a_word_that_holds_limit_is_no_refusal() {
    let stale = Activity {
        comments: vec![comment(
            "Codex Review: the usage limit is reached in `rate.rs`.\n\n**Reviewed commit:** `aaaaaaaaaa`",
            200,
        )],
        ..Activity::default()
    };
    assert_eq!(
        Codex.read(&stale, "bbbbbbbbbb", Timestamp(100)),
        Reading::Processing
    );
    for text in [
        "Delimiters reached the parser.",
        "An unlimited plan exceeded nothing.",
        "The rate-limited path was reached.",
    ] {
        assert!(!refused(text), "{text}");
    }
    assert!(refused(
        "You have reached your Codex usage limits for code reviews."
    ));
    assert!(refused("Review limit exceeded."));
}
