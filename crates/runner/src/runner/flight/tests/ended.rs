//! The reads the end of a worker's turn makes, run with the runner's lock let go

use super::*;
use crate::test::Scripted;

// The turn's end looks for the pull request the branch opened: the listing waits its turn behind
// the login's pace, so it is made with the lock let go and a trigger sent meanwhile is answered.
#[test]
fn a_trigger_is_answered_while_a_turns_end_looks_for_its_pull_request() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude
        .script([Scripted::Push("work.txt", "work\n"), Scripted::Text("done")]);
    let read = rig.forge.hold_next_open_pull_requests_read();

    let (reply, stepped) =
        rig.during_read(read, || step(&runner), || rig.ask(&runner, "status", None));

    assert!(reply.is_some(), "the trigger waited for the listing");
    stepped.unwrap();
    let item = lock(&runner).state.item(ticket(7)).cloned().unwrap();
    assert_eq!(item.pull_request, Some(71));
}
