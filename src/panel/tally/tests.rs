use super::*;

fn ballot(voter: Voter, choice: Option<char>, design: bool) -> Ballot {
    let vote = choice.map(|choice| Vote {
        choice,
        why: "because".into(),
        design,
    });
    Ballot { voter, vote }
}

fn votes(w: Option<char>, p: Option<char>, r: Option<char>, a: Option<char>) -> Vec<Ballot> {
    vec![
        ballot(Voter::Worker, w, false),
        ballot(Voter::Pm, p, false),
        ballot(Voter::Researcher, r, false),
        ballot(Voter::Advisor, a, false),
    ]
}

fn advice(choice: char, agree: u32, of: u32) -> Advice {
    Advice { choice, agree, of }
}

#[test]
fn a_type_sets_who_is_asked() {
    let tier = |title: &str| work_of(Some(title)).tier;
    assert_eq!(tier("docs(readme): say more"), Tier::Docs);
    for fix in [
        "fix: x",
        "refactor(runner): x",
        "perf: x",
        "test: x",
        "ci: x",
    ] {
        assert_eq!(tier(fix), Tier::Fix, "{fix}");
    }
    for fix in [
        "chore: x",
        "style: x",
        "fix(guard): let a project say its name",
    ] {
        assert_eq!(tier(fix), Tier::Fix, "{fix}");
    }
    assert_eq!(tier("feat(panel): decide"), Tier::Full);
    assert_eq!(tier("fix!: change the state file"), Tier::Full);
    assert_eq!(tier("docs(x)!: drop a page"), Tier::Full);
    assert_eq!(tier("Add a thing"), Tier::Full);
    assert_eq!(tier("wip: thing"), Tier::Full);
    assert_eq!(tier("fix(unclosed: x"), Tier::Full);
    assert_eq!(work_of(None).tier, Tier::Full);
}

#[test]
fn a_type_is_named_for_the_ledger() {
    let kind = |title: Option<&str>| work_of(title).kind;
    assert_eq!(kind(Some("fix(guard): x")), "fix");
    assert_eq!(kind(Some("feat!: x")), "breaking");
    assert_eq!(kind(Some("Add a thing")), "untyped");
    assert_eq!(kind(None), "untyped");
}

#[test]
fn four_or_three_of_four_decide_and_two_of_four_do_not() {
    let full = Stage::Tier(Tier::Full);
    let four = votes(Some('A'), Some('A'), Some('A'), Some('A'));
    assert_eq!(
        settle(full, Tier::Full, &four),
        Step::Decided(advice('A', 4, 4))
    );
    let three = votes(Some('B'), Some('A'), Some('B'), Some('B'));
    assert_eq!(
        settle(full, Tier::Full, &three),
        Step::Decided(advice('B', 3, 4))
    );
    let two = votes(Some('A'), Some('B'), Some('A'), Some('B'));
    assert_eq!(
        settle(full, Tier::Full, &two),
        Step::Maintainer {
            advice: None,
            design: false
        }
    );
}

#[test]
fn an_abstention_counts_against_the_three() {
    let full = Stage::Tier(Tier::Full);
    let abstained = votes(None, Some('A'), Some('A'), None);
    assert_eq!(
        settle(full, Tier::Full, &abstained),
        Step::Maintainer {
            advice: None,
            design: false
        }
    );
    let one = votes(None, Some('A'), Some('A'), Some('A'));
    assert_eq!(
        settle(full, Tier::Full, &one),
        Step::Decided(advice('A', 3, 4))
    );
}

#[test]
fn a_design_flag_sends_it_to_the_maintainer_with_the_recommendation() {
    let mut four = votes(Some('A'), Some('A'), Some('A'), Some('A'));
    four[3] = ballot(Voter::Advisor, Some('A'), true);
    assert_eq!(
        settle(Stage::Tier(Tier::Full), Tier::Full, &four),
        Step::Maintainer {
            advice: Some(advice('A', 4, 4)),
            design: true
        }
    );
}

#[test]
fn a_smaller_panel_decides_only_when_all_it_asked_agree_with_no_design_and_a_flag_asks_the_full_one()
 {
    let docs = Stage::Tier(Tier::Docs);
    let pm = [ballot(Voter::Pm, Some('A'), false)];
    assert_eq!(
        settle(docs, Tier::Docs, &pm),
        Step::Decided(advice('A', 1, 1))
    );
    let flagged = [ballot(Voter::Pm, Some('A'), true)];
    assert_eq!(
        settle(docs, Tier::Docs, &flagged),
        Step::Up(Stage::Tier(Tier::Full))
    );
    let fix = Stage::Tier(Tier::Fix);
    let agree = [
        ballot(Voter::Pm, Some('B'), false),
        ballot(Voter::Advisor, Some('B'), false),
    ];
    assert_eq!(
        settle(fix, Tier::Fix, &agree),
        Step::Decided(advice('B', 2, 2))
    );
    let split = [
        ballot(Voter::Pm, Some('A'), false),
        ballot(Voter::Advisor, Some('B'), false),
    ];
    assert_eq!(
        settle(fix, Tier::Fix, &split),
        Step::Up(Stage::Tier(Tier::Full))
    );
    let silent = [ballot(Voter::Pm, Some('A'), false)];
    assert_eq!(
        settle(fix, Tier::Fix, &silent),
        Step::Up(Stage::Tier(Tier::Full)),
        "an advisor that gave no vote is not agreement"
    );
}

#[test]
fn the_project_manager_alone_is_followed_by_the_panel_the_type_sets_or_the_full_one_on_a_flag() {
    let alone = Stage::Alone;
    let pm = [ballot(Voter::Pm, Some('A'), false)];
    assert_eq!(
        settle(alone, Tier::Full, &pm),
        Step::Decided(advice('A', 1, 1))
    );
    let abstained = [ballot(Voter::Pm, None, false)];
    assert_eq!(
        settle(alone, Tier::Fix, &abstained),
        Step::Up(Stage::Tier(Tier::Fix))
    );
    let flagged = [ballot(Voter::Pm, Some('A'), true)];
    assert_eq!(
        settle(alone, Tier::Docs, &flagged),
        Step::Up(Stage::Tier(Tier::Full))
    );
}

#[test]
fn simulating_asks_each_stage_up_until_one_decides() {
    let ballots = votes(Some('A'), Some('A'), Some('B'), Some('A'));
    assert_eq!(
        simulate(Stage::Tier(Tier::Docs), Tier::Docs, &ballots),
        (Stage::Tier(Tier::Docs), Step::Decided(advice('A', 1, 1)))
    );
    let split = votes(Some('A'), Some('A'), Some('B'), Some('B'));
    assert_eq!(
        simulate(Stage::Tier(Tier::Fix), Tier::Fix, &split),
        (
            Stage::Tier(Tier::Full),
            Step::Maintainer {
                advice: None,
                design: false
            }
        )
    );
}

#[test]
fn always_decides_a_two_two_split_by_the_pm_s_choice() {
    let split = votes(Some('A'), Some('B'), Some('A'), Some('B'));
    assert_eq!(
        settle_always(&split, false),
        Step::Decided(advice('B', 2, 4))
    );
    let other = votes(Some('B'), Some('A'), Some('B'), Some('A'));
    assert_eq!(
        settle_always(&other, false),
        Step::Decided(advice('A', 2, 4))
    );
    let most = votes(Some('A'), Some('B'), Some('A'), Some('C'));
    assert_eq!(
        settle_always(&most, false),
        Step::Decided(advice('A', 2, 4)),
        "the most votes win before the project manager is heard"
    );
}

#[test]
fn always_decides_a_tie_the_pm_is_not_in_by_the_first_option_listed() {
    let tied = votes(Some('C'), Some('A'), Some('B'), Some('C'));
    assert_eq!(
        settle_always(&tied, false),
        Step::Decided(advice('C', 2, 4))
    );
    let silent = votes(Some('C'), None, Some('B'), Some('B'));
    assert_eq!(
        settle_always(&silent, false),
        Step::Decided(advice('B', 2, 4))
    );
    let apart = votes(Some('C'), None, Some('B'), None);
    assert_eq!(
        settle_always(&apart, false),
        Step::Decided(advice('B', 1, 4)),
        "one vote each goes to the option listed first"
    );
}

#[test]
fn always_never_merges_on_a_tie() {
    let pm_for_merge = votes(Some('B'), Some('A'), Some('A'), Some('B'));
    assert_eq!(
        settle_always(&pm_for_merge, true),
        Step::Decided(advice('B', 2, 4))
    );
    let silent = votes(Some('B'), None, Some('A'), None);
    assert_eq!(
        settle_always(&silent, true),
        Step::Decided(advice('B', 1, 4))
    );
    let won = votes(Some('A'), Some('A'), Some('B'), None);
    assert_eq!(
        settle_always(&won, true),
        Step::Decided(advice('A', 2, 4)),
        "a majority for merging is not a tie"
    );
}

#[test]
fn always_decides_past_a_design_flag() {
    let mut four = votes(Some('A'), Some('A'), Some('A'), Some('A'));
    four[3] = ballot(Voter::Advisor, Some('A'), true);
    assert_eq!(
        settle_always(&four, false),
        Step::Decided(advice('A', 4, 4))
    );
}

#[test]
fn always_asks_the_maintainer_when_no_expert_voted() {
    let none = votes(None, None, None, None);
    assert_eq!(
        settle_always(&none, false),
        Step::Maintainer {
            advice: None,
            design: false
        }
    );
    assert_eq!(
        settle_always(&[], true),
        Step::Maintainer {
            advice: None,
            design: false
        }
    );
}
