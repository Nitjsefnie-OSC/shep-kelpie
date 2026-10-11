//! How the panel stands over the rulings the maintainer answered, read from
//! the lines of `rulings.jsonl` as a runner wrote them

use serde_json::{Value, json};

use super::*;

/// A ruling of `kind` answered `chose` by `by`, with the ballots `votes`
/// gives in the comment's order: the worker, the project manager, the
/// researcher and the advisor, each a letter, `-` for an abstention, or a
/// letter then `!` for one that flagged design. `advice` is the option the
/// panel stood behind, if any.
fn answered(
    id: u64,
    kind: &str,
    by: &str,
    chose: Option<char>,
    votes: &str,
    advice: Option<char>,
) -> Value {
    let voters = ["worker", "pm", "researcher", "advisor"];
    let ballots: Vec<Value> = voters
        .iter()
        .zip(votes.split(' '))
        .map(|(voter, vote)| match vote {
            "-" => json!({"voter": voter, "vote": null}),
            vote => json!({"voter": voter, "vote": {
                "choice": &vote[..1], "why": "because", "design": vote.ends_with('!'),
            }}),
        })
        .collect();
    let mut line = json!({
        "id": id, "at": 100 + id, "issue": 7, "pull_request": 71,
        "asked": "Merge it?", "answer": "yes", "kind": kind,
        "verdict": {"work": "feat", "tier": "full"},
        "ballots": ballots,
    });
    if let Some(chose) = chose {
        line["chose"] = json!(chose.to_string());
    }
    if let Some(advice) = advice {
        line["verdict"]["advice"] = json!({"choice": advice.to_string(), "agree": 3, "of": 4});
    }
    match by {
        "pm" => line["by_pm"] = json!(true),
        "panel" => line["by_panel"] = json!(true),
        _ => {}
    }
    line
}

fn standing_of(lines: &[Value]) -> Standing {
    let records: Vec<Record> = lines
        .iter()
        .map(|line| serde_json::from_value(line.clone()).unwrap())
        .collect();
    standing(Decide::Shadow, &records)
}

#[test]
fn the_streak_counts_each_answer_the_recommendation_matched_and_resets_on_a_miss() {
    let hit = |id| answered(id, "merge", "maintainer", Some('A'), "A A A A", Some('A'));
    let miss = |id| answered(id, "merge", "maintainer", Some('B'), "A A A A", Some('A'));

    assert_eq!(standing_of(&[hit(1), hit(2), hit(3)]).streak, 3);
    assert_eq!(standing_of(&[hit(1), hit(2), miss(3)]).streak, 0);
    assert_eq!(standing_of(&[miss(1), hit(2), hit(3)]).streak, 2);
}

#[test]
fn a_ruling_with_no_recommendation_leaves_the_streak_as_it_is() {
    let hit = |id| answered(id, "merge", "maintainer", Some('A'), "A A A A", Some('A'));
    let split = answered(2, "merge", "maintainer", Some('B'), "A B A B", None);

    assert_eq!(standing_of(&[hit(1), split, hit(3)]).streak, 2);
}

#[test]
fn an_answer_in_words_that_is_no_option_breaks_the_streak() {
    let hit = answered(1, "question", "maintainer", Some('A'), "A A A A", Some('A'));
    let other = answered(2, "question", "maintainer", None, "A A A A", Some('A'));

    assert_eq!(standing_of(&[hit, other]).streak, 0);
}

#[test]
fn only_the_maintainers_answers_count_toward_the_streak() {
    let hit = answered(1, "merge", "maintainer", Some('A'), "A A A A", Some('A'));
    let by_pm = answered(2, "question", "pm", Some('B'), "A A A A", Some('A'));
    let by_panel = answered(3, "still-red", "panel", Some('B'), "A A A A", Some('A'));

    let stand = standing_of(&[hit, by_pm, by_panel]);

    assert_eq!((stand.streak, stand.answered, stand.voted), (1, 1, 3));
    assert_eq!(stand.decided, 1);
}

#[test]
fn the_project_managers_vote_has_a_run_for_each_kind_of_ruling() {
    let pm_hit = |id, kind| answered(id, kind, "maintainer", Some('A'), "B A B B", None);
    let pm_miss = |id, kind| answered(id, kind, "maintainer", Some('B'), "B A B B", None);
    let flagged = answered(9, "still-red", "maintainer", Some('B'), "B A! B B", None);

    let stand = standing_of(&[
        pm_hit(1, "still-red"),
        pm_hit(2, "still-red"),
        pm_hit(3, "turn-timeout"),
        pm_miss(4, "turn-timeout"),
        flagged,
    ]);

    assert_eq!(
        stand.pm_alone["still-red"], 2,
        "a design flag leaves the run"
    );
    assert_eq!(stand.pm_alone["turn-timeout"], 0);
}

#[test]
fn an_experts_agreement_and_lone_correct_count_follow_the_answer() {
    // The researcher alone voted for the no the maintainer gave.
    let lone = answered(1, "merge", "maintainer", Some('B'), "A A B A", Some('A'));
    // The worker abstained, and the other three agreed with the answer.
    let shared = answered(2, "merge", "maintainer", Some('A'), "- A A A", Some('A'));

    let stand = standing_of(&[lone, shared]);

    assert_eq!(
        stand.experts["researcher"],
        Expert {
            votes: 2,
            abstained: 0,
            agreed: 2,
            lone: 1
        }
    );
    assert_eq!(
        stand.experts["worker"],
        Expert {
            votes: 1,
            abstained: 1,
            agreed: 0,
            lone: 0
        }
    );
    assert_eq!(
        stand.experts["pm"],
        Expert {
            votes: 2,
            abstained: 0,
            agreed: 1,
            lone: 0
        }
    );
}

#[test]
fn each_smaller_panel_is_scored_on_what_it_would_have_decided_alone() {
    // The project manager alone would have said A, and was right; the project
    // manager and the advisor split; all four decided on B, and were wrong.
    let first = answered(1, "merge", "maintainer", Some('A'), "B A B B", Some('B'));
    let second = answered(2, "merge", "maintainer", Some('B'), "B A B A", None);

    let stand = standing_of(&[first, second]);

    assert_eq!(
        stand.panels["pm"],
        Panel {
            decided: 2,
            matched: 1
        }
    );
    assert_eq!(
        stand.panels["pm+advisor"],
        Panel {
            decided: 1,
            matched: 0
        }
    );
    assert_eq!(
        stand.panels["all"],
        Panel {
            decided: 1,
            matched: 0
        }
    );
}

#[test]
fn the_facts_entry_is_due_after_thirty_rulings_the_panel_voted_on() {
    let ruling = |id| answered(id, "merge", "maintainer", Some('A'), "A A A A", Some('A'));
    let lines: Vec<Value> = (1..=30).map(ruling).collect();

    assert!(!standing_of(&lines[..29]).facts_due);
    assert!(standing_of(&lines).facts_due);
}
