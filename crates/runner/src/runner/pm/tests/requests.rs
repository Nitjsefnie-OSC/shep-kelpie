//! A request of the project manager that outlives its ruling or its runner

use std::time::Duration;

use super::asked::{advanced_until, approaches, comments, decided, foreign_put, pending, vote};
use crate::ports::Role;
use crate::test::{Hold, Rig, Scripted};

// A ruling raised again under the same id while the project manager works

/// When ruling 71 was raised, as the state keeps it
fn raised(rig: &Rig) -> serde_json::Value {
    let text = std::fs::read_to_string(rig.paths().state).unwrap();
    let state: serde_json::Value = serde_json::from_str(&text).unwrap();
    state["rulings"][0]["raised"].clone()
}

#[test]
fn an_answer_for_a_ruling_raised_again_meanwhile_is_dropped_and_says_so() {
    let asks = [
        (
            "asked-again-recommend",
            "71 pm recommend",
            vote('A', "PM-WHY"),
        ),
        (
            "asked-again-options",
            "71 pm options",
            approaches("Keep the old flag"),
        ),
        ("asked-again-decide", "71 pm decide", decided('A', false)),
    ];
    for (project, reply, said) in asks {
        let (rig, runner) = foreign_put(project);
        let hold = Hold::default();
        rig.claude
            .script_for(Role::Pm, [Scripted::HoldThenSay(hold.clone(), said)]);
        rig.writes_on(71, "octocat", reply);
        rig.next_look();
        advanced_until(&runner, || hold.entered(Duration::ZERO));
        let first = raised(&rig);

        // A yes accepts the label; its removal is another change, ruled again.
        rig.ask(&runner, "rule", Some("71 yes"));
        rig.clock.advance(5);
        rig.forge.unlabel_pull_request(71, "bug");
        advanced_until(&runner, || pending(&rig) && raised(&rig) != first);
        hold.release();
        let dropped = || {
            comments(&rig)
                .into_iter()
                .find(|c| c.contains("was dropped"))
        };
        advanced_until(&runner, || dropped().is_some());

        let said = dropped().unwrap();
        assert!(
            said.contains("about ruling #71 was dropped: the ruling was raised again"),
            "{project}: {said}"
        );
        let acted = |c: &String| {
            c.contains("**Project manager") || c.starts_with("**Answered by the project manager")
        };
        assert!(
            !comments(&rig).iter().any(acted),
            "{project}: {:?}",
            comments(&rig)
        );
        assert!(pending(&rig), "{project}: the new ruling still waits");
        let text = std::fs::read_to_string(rig.paths().state).unwrap();
        assert!(!text.contains("approaches"), "{project}: {text}");
    }
}

#[test]
fn pm_decide_is_checked_again_when_its_answer_comes() {
    let (rig, runner) = foreign_put("asked-decide-drained");
    let hold = Hold::default();
    rig.claude.script_for(
        Role::Pm,
        [Scripted::HoldThenSay(hold.clone(), decided('A', false))],
    );
    rig.writes_on(71, "octocat", "71 pm decide");
    rig.next_look();
    advanced_until(&runner, || hold.entered(Duration::ZERO));

    rig.ask(&runner, "drain", None);
    hold.release();
    let left = || {
        comments(&rig)
            .into_iter()
            .find(|c| c.contains("was not applied"))
    };
    advanced_until(&runner, || left().is_some());

    assert!(
        left().unwrap().contains("since the runner drains"),
        "{:?}",
        left()
    );
    assert!(pending(&rig));
}
