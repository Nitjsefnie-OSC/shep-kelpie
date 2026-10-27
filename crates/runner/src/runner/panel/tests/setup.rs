//! A worker's turn kelpie could not set up goes to the maintainer with no
//! panel, since a retry does not fix kelpie's own files (shep-pm/shep-kelpie#1368)

use super::{name_panel, script_all, seen, state_of};
use crate::ports::{AgentError, Role};
use crate::runner::{StepReport, step};
use crate::test::ticket;
use crate::test::{CALL_PATIENCE, Rig, Scripted};

const WHY: &str = "cannot empty worker/settings-7.tmp: permission denied";

#[test]
fn a_turn_kelpie_could_not_set_up_goes_to_the_maintainer_with_no_panel() {
    let rig = Rig::new("spheal");
    rig.with_app();
    rig.maintainer("octocat");
    name_panel(&rig, true);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude
        .script([Scripted::Fail(AgentError::Setup(WHY.into()))]);
    let first = step(&runner).unwrap();
    assert!(
        matches!(&first, Some(StepReport::Failed { id, .. }) if *id == ticket(7)),
        "{first:?}"
    );
    script_all(&rig, true);

    let deadline = std::time::Instant::now() + CALL_PATIENCE;
    let comment = loop {
        step(&runner).unwrap();
        if let Some((_, body)) = rig.github.comments().into_iter().next() {
            break body;
        }
        assert!(std::time::Instant::now() < deadline, "no ruling was posted");
        std::thread::yield_now();
    };

    assert!(
        comment.starts_with("### Kelpie could not set up the worker's turn."),
        "{comment}"
    );
    assert!(!comment.contains("**Panel**"), "{comment}");
    assert!(
        !comment.contains("settings-7.tmp"),
        "the path stays off the forge: {comment}"
    );
    for role in [Role::Advisor, Role::Researcher, Role::Pm] {
        assert!(seen(&rig, role).is_empty(), "{role:?} was asked");
    }
    let ruling = &state_of(&rig)["rulings"][0];
    assert_eq!(
        ruling["question"],
        format!(
            "Kelpie could not set up the worker's turn on issue #7: {WHY}. Fix it on this \
             machine, then `shep kelpie rule 7 yes` tries that step again, and `shep kelpie \
             rule 7 no <note>` stops the work item."
        )
    );
    assert_eq!(ruling["skipped"], "setup", "{ruling}");
    assert_eq!(ruling.get("panel"), None, "{ruling}");
}
