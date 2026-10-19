//! `add`, `gate`, `drop`, `attach` and `detach` naming a ticket by id

use super::*;

#[test]
fn add_takes_one_plain_ticket_id() {
    let rig = Rig::new("golbat");
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "add", None),
        json!({ "error": "`add` takes a ticket id" })
    );
    for bad in ["#7", "0", "-7", "+7", "7 8", "seven", "k0", "K12"] {
        assert_eq!(
            rig.ask(&runner, "add", Some(bad)),
            json!({ "error": format!("{bad:?} is not a ticket id") }),
        );
    }
    assert_eq!(rig.ask(&runner, "status", None)["work_item"], json!(null));
}

#[test]
fn add_reads_a_local_ticket_id_but_opens_nothing_for_it() {
    let rig = Rig::new("golbat");
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "add", Some("k12")),
        json!({ "error": "k12 is not a GitHub issue, the only tickets kelpie reads" })
    );
    assert_eq!(rig.ask(&runner, "status", None)["work_item"], json!(null));
}

#[test]
fn a_local_ticket_has_no_work_item_open() {
    let (rig, runner) = with_issue_7("golbat");
    for action in ["gate", "drop"] {
        assert_eq!(
            rig.ask(&runner, action, Some("k12")),
            json!({ "error": "no work item for k12 is open" }),
            "{action}"
        );
    }
    let me = std::process::id();
    for (action, params) in [
        ("attach", format!("k12 {me}")),
        ("detach", format!("k12 {me}")),
    ] {
        assert_eq!(
            rig.ask(&runner, action, Some(&params)),
            json!({ "error": "no work item for k12 is open" }),
            "{action}"
        );
    }
}
