//! How the panel stands over the rulings the maintainer answered, read from
//! the lines of `rulings.jsonl` as a runner wrote them

use serde_json::{Value, json};

use crate::usage::{Line, vote_costs};

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
    standing(Decide::Shadow, &records, &BTreeMap::new())
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

    // Votes, abstentions, agreements and lone ones.
    let counts = |name: &str| {
        let e = &stand.experts[name];
        (e.votes, e.abstained, e.agreed, e.lone)
    };
    assert_eq!(counts("researcher"), (2, 0, 2, 1));
    assert_eq!(counts("worker"), (1, 1, 0, 0));
    assert_eq!(counts("pm"), (2, 0, 1, 0));
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

#[test]
fn an_experts_agreement_and_lone_correct_are_split_by_kind_over_the_maintainers_answers_only() {
    let by_maintainer =
        |id, kind, chose, votes| answered(id, kind, "maintainer", Some(chose), votes, Some('A'));
    let stand = standing_of(&[
        // The researcher alone was right on a merge, and the advisor on a question.
        by_maintainer(1, "merge", 'B', "A A B A"),
        by_maintainer(2, "question", 'A', "B B B A"),
        by_maintainer(3, "merge", 'A', "A A A A"),
        // Neither the project manager's nor the panel's answer is the maintainer's.
        answered(4, "merge", "pm", Some('A'), "A A A A", Some('A')),
        answered(5, "question", "panel", Some('B'), "B B B B", Some('B')),
    ]);

    let researcher = &stand.experts["researcher"].by_kind;
    assert_eq!(researcher["merge"], Agreement { agreed: 2, lone: 1 });
    assert_eq!(researcher["question"], Agreement::default());
    let advisor = &stand.experts["advisor"].by_kind;
    assert_eq!(advisor["merge"], Agreement { agreed: 1, lone: 0 });
    assert_eq!(advisor["question"], Agreement { agreed: 1, lone: 1 });
    assert_eq!(
        researcher.len(),
        2,
        "no row for a kind only others answered"
    );
}

/// A ruling answered under shadow by the maintainer, whose work item's type set
/// panel `tier`, with the ballots `votes` gives
fn shadowed(id: u64, kind: &str, tier: &str, risky: bool, chose: char, votes: &str) -> Value {
    let mut line = answered(id, kind, "maintainer", Some(chose), votes, None);
    line["verdict"]["tier"] = json!(tier);
    line["verdict"]["shadow"] = json!(true);
    if risky {
        line["verdict"]["risky"] = json!(true);
    }
    line
}

/// The ledger line of a call that cost `usd` and cast `voter`'s vote on each
/// of `rulings`, a wake for the project manager
fn vote_call(voter: &str, rulings: &[u64], usd: f64) -> Line {
    let marks: Vec<Value> = (rulings.iter())
        .map(|ruling| {
            json!({
                "ruling": ruling, "ruling_kind": "merge", "voter": voter,
                "panel": "fix", "work": "feat",
            })
        })
        .collect();
    let call = serde_json::from_value(json!({
        "at": 100, "issue": null, "role": voter,
        "kind": if voter == "pm" { "wake" } else { "vote" },
        "usage": { "input": 10, "cache_write": 0, "cache_read": 0, "output": 0 },
        "units": 10, "cost_usd": usd, "seconds": 5, "ended": "answered", "votes": marks,
    }))
    .unwrap();
    Line::Call(call)
}

fn whatif_of(lines: &[Value], calls: &[Line]) -> BTreeMap<String, Score> {
    let records: Vec<Record> = (lines.iter())
        .map(|line| serde_json::from_value(line.clone()).unwrap())
        .collect();
    standing(Decide::Shadow, &records, &vote_costs(calls)).whatif
}

fn assert_score(got: &Score, (decided, matched, maintainer): (u32, u32, u32), cost: f64) {
    assert_eq!(
        (got.decided, got.matched, got.maintainer),
        (decided, matched, maintainer)
    );
    assert!(
        (got.cost - cost).abs() < 1e-9,
        "cost {} is not {cost}",
        got.cost
    );
}

// Four shadow rulings, voted worker, project manager, researcher, advisor.
// Every expert's vote costs what it did on each, the project manager's wake
// for rulings 1 and 2 sharing its $0.20 between them.
//
//   1  merge     docs  "A A A A"  maintainer A
//   2  merge     fix   "B A B A"  maintainer B
//   3  question  fix   "A B A B"  maintainer B
//   4  merge     docs, risky path  "A A A B"  maintainer A
//
// Worker $0.40, project manager $0.10, researcher $0.30, advisor $0.20.
fn recorded() -> (Vec<Value>, Vec<Line>) {
    let mut lines = vec![
        shadowed(1, "merge", "docs", false, 'A', "A A A A"),
        shadowed(2, "merge", "fix", false, 'B', "B A B A"),
        shadowed(3, "question", "fix", false, 'B', "A B A B"),
        shadowed(4, "merge", "docs", true, 'A', "A A A B"),
    ];
    // Not scored: the project manager answered it, the verdict is from before
    // the votes were kept as shadow, and no vote was cast.
    lines.push(answered(5, "merge", "pm", Some('A'), "A A A A", None));
    lines.push(answered(
        6,
        "merge",
        "maintainer",
        Some('A'),
        "A A A A",
        None,
    ));
    let mut unvoted = answered(7, "merge", "maintainer", Some('A'), "A A A A", None);
    unvoted["ballots"] = json!([]);
    lines.push(unvoted);
    let mut calls = vec![
        vote_call("pm", &[1, 2], 0.20),
        vote_call("pm", &[3], 0.10),
        vote_call("pm", &[4], 0.10),
        vote_call("pm", &[5, 6], 9.0),
    ];
    for ruling in [1, 2, 3, 4, 5] {
        calls.push(vote_call("worker", &[ruling], 0.40));
        calls.push(vote_call("researcher", &[ruling], 0.30));
        calls.push(vote_call("advisor", &[ruling], 0.20));
    }
    (lines, calls)
}

#[test]
fn the_type_policy_decides_each_ruling_with_the_panel_its_type_sets() {
    let (lines, calls) = recorded();
    let whatif = whatif_of(&lines, &calls);

    // Docs asks the project manager, fix adds the advisor; both agreed each time.
    // (1 + 2 + 3 + 4) cost: $0.10 + $0.30 + $0.30 + $0.10, over four rulings.
    assert_score(&whatif["type"], (4, 3, 0), 0.20);
}

#[test]
fn type_and_risk_sends_a_question_and_a_risky_path_to_all_four_and_a_split_to_the_maintainer() {
    let (lines, calls) = recorded();
    let whatif = whatif_of(&lines, &calls);

    // Ruling 3 splits two to two, so the maintainer has it; ruling 4 is three
    // to one. $0.10 + $0.30 + $1.00 + $1.00, over four rulings.
    assert_score(&whatif["type+risk"], (3, 2, 1), 0.60);
}

#[test]
fn kind_and_risk_asks_the_advisor_of_a_plain_ruling_and_everyone_on_a_question() {
    let (lines, calls) = recorded();
    let whatif = whatif_of(&lines, &calls);

    // $0.30 + $0.30 + $1.00 + $1.00, over four rulings.
    assert_score(&whatif["kind+risk"], (3, 2, 1), 0.65);
}

#[test]
fn an_escalation_costs_the_votes_it_asked_on_top_of_the_pairs() {
    let (lines, calls) = recorded();
    let whatif = whatif_of(&lines, &calls);

    // The project manager and the researcher disagree on ruling 2 and the
    // full panel splits, so it is asked all four and still goes to the
    // maintainer: $0.40 + $1.00 + $1.00 + $1.00, over four rulings.
    assert_score(&whatif["kind+risk-researcher"], (2, 2, 2), 0.85);
}

#[test]
fn the_fixed_pairs_and_all_four_are_scored_the_same_way() {
    let (lines, calls) = recorded();
    let whatif = whatif_of(&lines, &calls);

    assert_score(&whatif["pm+advisor"], (3, 2, 1), 0.30);
    assert_score(&whatif["pm+researcher"], (2, 2, 2), 0.40);
    assert_score(&whatif["all"], (2, 2, 2), 1.00);
    let names: Vec<&String> = whatif.keys().collect();
    assert_eq!(names.len(), 7, "{names:?}");
}

#[test]
fn nothing_is_scored_before_a_ruling_is_answered_under_shadow() {
    let (lines, calls) = recorded();
    let whatif = whatif_of(&lines[4..], &calls);

    for score in whatif.values() {
        assert_score(score, (0, 0, 0), 0.0);
    }
}
