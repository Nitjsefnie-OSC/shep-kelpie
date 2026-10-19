//! `board`, `board move` and `board top` through the trigger

use super::*;

// Three issues ready and blocked by #32, so a read of the board lists them
// and takes none.
fn three_ready(rig: &Rig, runner: &Mutex<Runner>) {
    for issue in [282, 284, 285] {
        rig.forge.list_ready(issue, false);
        rig.forge.block(issue, 32);
    }
    assert_eq!(step(runner).unwrap(), None);
}

// The issues `board` lists, in order, after `params`.
fn listed(rig: &Rig, runner: &Mutex<Runner>, params: Option<&str>) -> Vec<u64> {
    let answer = rig.ask(runner, "board", params);
    let list = answer["board"].as_array().expect("a list");
    list.iter()
        .map(|e| e["issue"].as_str().unwrap().parse().unwrap())
        .collect()
}

fn t(number: u64) -> TicketId {
    crate::test::ticket(number)
}

#[test]
fn board_move_makes_the_runner_take_the_issue_at_its_new_place() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    three_ready(&rig, &runner);
    assert_eq!(listed(&rig, &runner, Some("move 282 3")), [284, 285, 282]);

    rig.forge.close_issue(32);
    // One slot, so each work item is dropped to free it for the next pick.
    let mut taken = Vec::new();
    for _ in 0..3 {
        rig.next_look();
        let Some(StepReport::Dispatched { issue, .. }) = step(&runner).unwrap() else {
            break;
        };
        taken.push(issue);
        rig.ask(&runner, "drop", Some(&issue.to_string()));
    }
    assert_eq!(taken, [t(284), t(285), t(282)]);
}

#[test]
fn board_top_puts_the_issue_first() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    three_ready(&rig, &runner);
    assert_eq!(listed(&rig, &runner, Some("top 285")), [285, 282, 284]);
    assert_eq!(
        rig.ask(&runner, "status", None)["order"],
        json!([285, 282, 284])
    );
}

#[test]
fn a_move_leaves_an_open_issue_between_the_listed_ones_where_it_was() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    three_ready(&rig, &runner);
    // #284 is open, so the list leaves it out, and the order still holds it.
    rig.ask(&runner, "add", Some("284"));
    assert_eq!(listed(&rig, &runner, None), [282, 285]);
    assert_eq!(listed(&rig, &runner, Some("top 285")), [285, 282]);
    assert_eq!(
        rig.ask(&runner, "status", None)["order"],
        json!([285, 284, 282])
    );
}

#[test]
fn board_lists_each_issue_with_its_place_title_priority_and_why_it_waits() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    rig.forge.label(284, "priority: P0");
    three_ready(&rig, &runner);
    assert_eq!(
        rig.ask(&runner, "board", None),
        json!({
            "project": "koji",
            "board": [
                { "position": 1, "issue": "284", "title": "Title of #284",
                  "priority": "P0", "blocked": true, "held": false },
                { "position": 2, "issue": "282", "title": "Title of #282",
                  "priority": null, "blocked": true, "held": false },
                { "position": 3, "issue": "285", "title": "Title of #285",
                  "priority": null, "blocked": true, "held": false },
            ],
        })
    );
}

#[test]
fn a_move_of_an_issue_that_is_not_ready_is_refused_and_the_list_is_unchanged() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    three_ready(&rig, &runner);
    for params in ["move 290 1", "top 290"] {
        assert_eq!(
            rig.ask(&runner, "board", Some(params)),
            json!({ "error": "#290 is not ready, so it is not in the list" })
        );
    }
    assert_eq!(listed(&rig, &runner, None), [282, 284, 285]);
    assert_eq!(
        rig.ask(&runner, "status", None)["order"],
        json!([282, 284, 285])
    );
}

#[test]
fn a_position_past_the_end_is_refused_and_the_list_is_unchanged() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    three_ready(&rig, &runner);
    assert_eq!(
        rig.ask(&runner, "board", Some("move 282 4")),
        json!({ "error": "position 4 is past the end of the list, which holds 3 issues" })
    );
    assert_eq!(
        rig.ask(&runner, "board", Some("move 282 0")),
        json!({ "error": "positions start at 1" })
    );
    assert_eq!(listed(&rig, &runner, None), [282, 284, 285]);
}

#[test]
fn board_refuses_params_it_cannot_read() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    for params in ["move 282", "top", "top 282 1", "move x 1", "up 282"] {
        let answer = rig.ask(&runner, "board", Some(params));
        let error = answer["error"].as_str().unwrap();
        assert!(error.starts_with("`board` takes"), "{params}: {error}");
    }
}
