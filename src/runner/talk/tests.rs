//! Comments from people on a pull request, through the runner's stand-ins

use serde_json::json;

use crate::ports::Standing;
use crate::runner::review_bot::LATE_READ;
use crate::ports::Checks;
use crate::runner::{StepReport, step};
use crate::test::{Rig, Scripted};

const ASK: &str = "Please rename the flag to `--check`.";

// A comment while a merge ruling waits is the maintainer's no, said on the
// pull request: the ruling goes, and the worker has the comment to act on.
#[test]
fn a_writers_comment_while_a_merge_ruling_waits_withdraws_it_and_reaches_the_worker() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    rig.forge
        .coderabbit
        .person_says(71, "octo-reviewer", Standing::Writer, ASK);
    rig.clock.advance(LATE_READ);
    assert_eq!(step(&runner).unwrap(), None, "the comment is read");
    assert_eq!(rig.ask(&runner, "status", None)["rulings"], json!([]));

    rig.claude
        .script([Scripted::Push("rename.txt", "renamed\n")]);
    step(&runner).unwrap(); // the worker's turn
    let prompt = rig.claude.seen().last().unwrap().call.prompt.clone();
    assert!(prompt.contains(ASK), "{prompt}");
    assert!(prompt.contains("octo-reviewer"), "{prompt}");
}

const COMMENT_FIX: &str =
    "This head is the worker's fix for a note or comment, and no reviewer read it.";

// A review thread from a writer reaches the worker as a numbered request.
// The worker's fix is answered on the thread as a bot's is, and comes back
// to a new merge ruling that says no reviewer read it.
#[test]
fn a_writers_review_thread_is_fixed_replied_to_and_resolved() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    let thread = rig.forge.coderabbit.person_opens(
        71,
        "src/lib.rs",
        "octo-reviewer",
        Standing::Writer,
        "This name is wrong.",
    );
    rig.clock.advance(LATE_READ);
    assert_eq!(step(&runner).unwrap(), None, "the thread is read");

    rig.claude.script([Scripted::PushSay(
        "rename.txt",
        "renamed\n",
        "Renamed it.\nthread 1: fixed",
    )]);
    step(&runner).unwrap(); // the worker's turn
    let prompt = rig.claude.seen().last().unwrap().call.prompt.clone();
    assert!(prompt.contains("This name is wrong."), "{prompt}");
    assert!(prompt.contains("src/lib.rs:3"), "{prompt}");
    assert_eq!(rig.ask(&runner, "status", None)["work_item"]["phase"]["state"], "ci");

    let fixed = rig.forge.head_of("kelpie/7").unwrap();
    rig.forge.set_checks(&fixed, Checks::Passed);
    let Some(StepReport::Ruling {
        id: 2, question, ..
    }) = rig.verdict(&runner)
    else {
        panic!("no merge ruling on the fix");
    };
    assert!(question.contains(COMMENT_FIX), "{question}");
    assert_eq!(
        rig.forge.coderabbit.replies(),
        [(thread.clone(), "Fixed.".to_owned())]
    );
    assert_eq!(rig.forge.coderabbit.resolved(), [thread]);
}
