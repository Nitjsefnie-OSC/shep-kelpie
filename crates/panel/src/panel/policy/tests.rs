use super::*;
use crate::panel::Vote;

/// The ballots `votes` gives in the order the worker, the project manager,
/// the researcher and the advisor: a letter each, `-` for an abstention, a
/// letter then `!` for one that flagged design
fn ballots(votes: &str) -> Vec<Ballot> {
    let voters = [Voter::Worker, Voter::Pm, Voter::Researcher, Voter::Advisor];
    voters
        .into_iter()
        .zip(votes.split(' '))
        .map(|(voter, vote)| Ballot {
            voter,
            vote: (vote != "-").then(|| Vote {
                choice: vote.chars().next().unwrap(),
                why: "because".into(),
                design: vote.ends_with('!'),
            }),
        })
        .collect()
}

/// What `policy` comes to on a ruling with `votes`, as who it asked (sorted)
/// and the option it decided on
fn play(
    policy: Policy,
    (question, risky, tier): (bool, bool, Tier),
    votes: &str,
) -> (Vec<Voter>, Option<char>) {
    let ballots = ballots(votes);
    let case = Case {
        question,
        risky,
        tier,
        ballots: &ballots,
    };
    let Outcome { mut asked, decided } = policy.play(&case);
    asked.sort();
    (asked, decided)
}

const PLAIN: (bool, bool, Tier) = (false, false, Tier::Fix);
const QUESTION: (bool, bool, Tier) = (true, false, Tier::Fix);
const RISKY: (bool, bool, Tier) = (false, true, Tier::Docs);

const ALL: [Voter; 4] = [Voter::Worker, Voter::Pm, Voter::Advisor, Voter::Researcher];
const PM_ADVISOR: [Voter; 2] = [Voter::Pm, Voter::Advisor];
const PM_RESEARCHER: [Voter; 2] = [Voter::Pm, Voter::Researcher];

#[test]
fn a_question_or_a_risky_path_starts_at_the_full_panel_and_nothing_starts_lower() {
    assert_eq!(first_tier(Tier::Docs, true, false), Tier::Full);
    assert_eq!(first_tier(Tier::Fix, false, true), Tier::Full);
    assert_eq!(first_tier(Tier::Docs, false, false), Tier::Docs);
    assert_eq!(first_tier(Tier::Fix, false, false), Tier::Fix);
    assert_eq!(first_tier(Tier::Full, false, false), Tier::Full);
}

#[test]
fn the_types_table_stops_at_the_panel_the_type_sets_when_it_agrees() {
    let got = play(Policy::Type, RISKY, "B A B B");
    assert_eq!(got, (vec![Voter::Pm], Some('A')));
}

#[test]
fn type_and_risk_asks_all_four_on_a_question_or_a_risky_path() {
    let votes = "B A B A";
    assert_eq!(
        play(Policy::Type, QUESTION, votes),
        (PM_ADVISOR.to_vec(), Some('A'))
    );
    assert_eq!(
        play(Policy::TypeRisk, QUESTION, votes),
        (ALL.to_vec(), None),
        "two of four is no majority"
    );
    assert_eq!(
        play(Policy::TypeRisk, RISKY, "A A A B"),
        (ALL.to_vec(), Some('A'))
    );
}

#[test]
fn the_kind_policies_ask_a_pair_and_go_up_to_the_full_panel_on_a_split() {
    let agree = "B A B A";
    let split = "A A A B";
    assert_eq!(
        play(Policy::KindRisk, PLAIN, agree),
        (PM_ADVISOR.to_vec(), Some('A'))
    );
    assert_eq!(
        play(Policy::KindRisk, PLAIN, split),
        (ALL.to_vec(), Some('A'))
    );
    assert_eq!(
        play(Policy::KindRiskResearcher, PLAIN, "B A A B"),
        (PM_RESEARCHER.to_vec(), Some('A'))
    );
    assert_eq!(
        play(Policy::KindRiskResearcher, PLAIN, "A A B B"),
        (ALL.to_vec(), None)
    );
}

#[test]
fn the_kind_policies_ask_all_four_for_a_question_a_risky_path_or_a_design_flag() {
    for policy in [Policy::KindRisk, Policy::KindRiskResearcher] {
        assert_eq!(play(policy, QUESTION, "A A A A").0, ALL, "{policy:?}");
        assert_eq!(play(policy, RISKY, "A A A A").0, ALL, "{policy:?}");
        // Flagged by the pair's project manager, which is not agreement.
        assert_eq!(
            play(policy, PLAIN, "A A! A A"),
            (ALL.to_vec(), None),
            "{policy:?}"
        );
    }
}

#[test]
fn a_fixed_pair_has_no_one_above_it() {
    assert_eq!(
        play(Policy::PmAdvisor, QUESTION, "A A A B"),
        (PM_ADVISOR.to_vec(), None)
    );
    assert_eq!(
        play(Policy::PmResearcher, PLAIN, "A A A B"),
        (PM_RESEARCHER.to_vec(), Some('A'))
    );
    assert_eq!(
        play(Policy::PmResearcher, PLAIN, "A A! A B"),
        (PM_RESEARCHER.to_vec(), None),
        "a design flag leaves the pair's ruling to the maintainer"
    );
}

#[test]
fn all_decides_on_three_of_four_and_an_abstention_is_no_vote() {
    assert_eq!(
        play(Policy::All, PLAIN, "A A B A"),
        (ALL.to_vec(), Some('A'))
    );
    assert_eq!(play(Policy::All, PLAIN, "A A - -"), (ALL.to_vec(), None));
}

#[test]
fn an_abstention_in_a_pair_is_not_agreement() {
    assert_eq!(
        play(Policy::PmAdvisor, PLAIN, "A A A -"),
        (PM_ADVISOR.to_vec(), None)
    );
}

#[test]
fn every_candidate_has_its_own_name() {
    let names: Vec<&str> = Policy::ALL.iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        [
            "type",
            "type+risk",
            "kind+risk",
            "kind+risk-researcher",
            "pm+advisor",
            "pm+researcher",
            "all"
        ]
    );
}
