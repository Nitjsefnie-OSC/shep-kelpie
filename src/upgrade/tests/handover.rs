//! Handing each runner over to the new build, and draining it when it cannot

use super::*;

// A runner still on the old build, and one that reports the installed file
const OLD: &str = r#"{"build":"1 1.000000000","work_items":[]}"#;
const NEW: &str = r#"{"build":"{build}","work_items":[]}"#;

const REFUSED: &str = r#"{"error":"handover refused: a call has just ended and is being read"}"#;

// A runner draining with no call left
const DRAINED: &str = r#"{"work_items":[],"draining":{"calls":[],"ceiling":3600}}"#;

// The triggers sent to `sheep`, by action
fn triggers(writes: &[Request], sheep: &str) -> Vec<String> {
    (writes.iter())
        .filter_map(|w| match w {
            Request::Trigger {
                selector: SelectorSpec::Name(n),
                action,
                ..
            } if n == sheep => Some(action.clone()),
            _ => None,
        })
        .collect()
}

fn restarted(writes: &[Request]) -> Vec<&str> {
    (writes.iter())
        .filter_map(|w| match w {
            Request::Restart {
                selector: SelectorSpec::Name(name),
            } => Some(name.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn upgrade_hands_over_a_runner_that_can_and_drains_none() {
    let mut rig = Rig::new().await;
    // The dog's look, then the old build until the handover lands. The old
    // image never answers `handover`: it is replaced as it takes it.
    rig.shepherd.says("koji", &[IDLE, OLD, NEW]);
    rig.shepherd.hands("koji", &[None]);
    let new = rig.build("new", "0.3.0", "0.13.0");
    let (ran, said) = rig.install(&new).await;
    ran.unwrap();

    let writes = rig.shepherd.writes();
    assert_eq!(restarted(&writes), ["kelpie"], "only the dog restarts");
    assert_eq!(
        triggers(&writes, "koji"),
        ["status", "handover", "status", "status"]
    );
    assert!(
        said.iter()
            .any(|l| l == "`koji` handed over to the new build"),
        "{said:?}"
    );
}

#[tokio::test]
async fn upgrade_drains_a_runner_that_predates_the_handover() {
    let mut rig = Rig::new().await;
    // An old build answers `status` and takes no `handover`, twice.
    rig.shepherd.says("koji", &[IDLE, IDLE, DRAINED]);
    rig.shepherd.hands(
        "koji",
        &[
            Some("unknown action: handover"),
            Some("unknown action: handover"),
        ],
    );
    let new = rig.build("new", "0.3.0", "0.13.0");
    let (ran, said) = rig.install(&new).await;
    ran.unwrap();

    let writes = rig.shepherd.writes();
    assert_eq!(restarted(&writes), ["kelpie", "koji"]);
    assert_eq!(
        triggers(&writes, "koji"),
        [
            "status", "handover", "status", "handover", "drain", "status", "status", "status"
        ]
    );
    assert!(
        said.iter().any(|l| l
            == "`koji` runs a kelpie from before `handover`, so it is drained and restarted as before"),
        "{said:?}"
    );
}

#[tokio::test]
async fn upgrade_drains_a_runner_that_refuses_and_names_the_reason() {
    let mut rig = Rig::new().await;
    rig.shepherd.says("koji", &[IDLE, DRAINED]);
    rig.shepherd.hands("koji", &[Some(REFUSED)]);
    let new = rig.build("new", "0.3.0", "0.13.0");
    let (ran, said) = rig.install(&new).await;
    ran.unwrap();

    let writes = rig.shepherd.writes();
    assert_eq!(restarted(&writes), ["kelpie", "koji"]);
    assert_eq!(
        triggers(&writes, "koji"),
        ["status", "handover", "drain", "status", "status", "status"]
    );
    assert!(
        said.iter().any(|l| l
            == "`koji` refused to hand over: a call has just ended and is being read, so it is \
                drained and restarted as before"),
        "{said:?}"
    );
}

#[tokio::test]
async fn a_handover_that_never_shows_the_new_build_is_drained_and_named() {
    let mut rig = Rig::new().await;
    rig.shepherd.says(
        "koji",
        &[IDLE, OLD, OLD, OLD, OLD, OLD, OLD, OLD, OLD, DRAINED],
    );
    rig.shepherd.hands(
        "koji",
        &[Some(
            r#"{"handover":"waiting for the runner's quiet point"}"#,
        )],
    );
    let new = rig.build("new", "0.3.0", "0.13.0");
    let patience = Patience {
        start: Duration::from_millis(30),
        ..FAST
    };
    let action = Action::Install(Source::Binary(new));
    let (ran, said) = rig.upgrade_within(action, patience).await;
    ran.unwrap();

    assert_eq!(restarted(&rig.shepherd.writes()), ["kelpie", "koji"]);
    assert!(
        said.iter().any(|l| l
            == "`koji` did not show the new build within 0s of its handover, so it is drained \
                and restarted as before"),
        "{said:?}"
    );
}
