//! Two runners on one login, one fake clock

use super::*;
use crate::forge_ledger::{self, Ledger};
use crate::ports::CallPacer;
use crate::test::{FakeClock, FakeForge};

// A runner of `project` on `clock`, spaced and held by `hold`.
fn runner(
    forge: &FakeForge,
    clock: &FakeClock,
    hold: &ForgeHold,
    project: &str,
    ledger: &Arc<Ledger>,
) -> RateHeld {
    RateHeld::new(
        Arc::new(forge.clone()),
        Arc::new(clock.clone()),
        hold.clone(),
        Box::new(|_| {}),
    )
    .with_ledger(Arc::clone(ledger))
    .paced_for(project)
}

#[test]
fn one_runners_used_up_answer_holds_the_other_runner() {
    let home = tempfile::tempdir().unwrap();
    let clock = FakeClock::at(1_000_000);
    let hold = ForgeHold::shared(Arc::new(LocalPace::new()));
    let (koji_forge, golbat_forge) = (
        FakeForge::new(home.path().join("koji")),
        FakeForge::new(home.path().join("golbat")),
    );
    let ledger = Arc::new(Ledger::in_home(home.path(), "x"));
    let (first, second) = (
        runner(&koji_forge, &clock, &hold, "koji", &ledger),
        runner(&golbat_forge, &clock, &hold, "golbat", &ledger),
    );
    koji_forge.set_used_up(Some(Timestamp(1_000_600)));

    first.viewer().unwrap_err();

    assert_eq!(
        second.viewer().unwrap_err(),
        ForgeError::Held(Timestamp(1_000_600))
    );
    assert_eq!(golbat_forge.asked(), 0, "the held runner never called");
    clock.advance(600);
    koji_forge.set_used_up(None);
    second.viewer().unwrap();
}

#[test]
fn calls_past_the_cap_wait_and_then_go_and_no_minute_is_over_it() {
    const CAP: usize = 6;
    let home = tempfile::tempdir().unwrap();
    let clock = FakeClock::at(1_000_000);
    let hold = ForgeHold::shared(Arc::new(LocalPace::with(CallPacer::new(CAP, 3))));
    let (koji_forge, golbat_forge) = (
        FakeForge::new(home.path().join("koji")),
        FakeForge::new(home.path().join("golbat")),
    );
    let ledger = Arc::new(Ledger::in_home(home.path(), "x"));
    let (first, second) = (
        runner(&koji_forge, &clock, &hold, "koji", &ledger),
        runner(&golbat_forge, &clock, &hold, "golbat", &ledger),
    );

    for _ in 0..20 {
        first.viewer().unwrap();
        second.viewer().unwrap();
    }

    assert_eq!(koji_forge.asked() + golbat_forge.asked(), 40, "none failed");
    assert!(clock.now().0 > 1_000_000 + 60, "the later calls waited");
    let lines = forge_ledger::read(&home.path().join(forge_ledger::FILE)).unwrap();
    let mut minutes = std::collections::BTreeMap::<u64, usize>::new();
    for line in &lines {
        *minutes.entry(line.at.0 / 60).or_default() += 1;
    }
    // The ledger marks a call by when it ended; these end as they start.
    let busiest = minutes.values().max().copied().unwrap_or(0);
    assert!(busiest <= CAP, "{minutes:?}");
    assert_eq!(lines.len(), 40);
}

#[test]
fn a_runner_with_no_project_to_pace_still_sees_the_hold() {
    let home = tempfile::tempdir().unwrap();
    let clock = FakeClock::at(1_000_000);
    let hold = ForgeHold::shared(Arc::new(LocalPace::new()));
    let forge = FakeForge::new(home.path().join("koji"));
    let unpaced = RateHeld::new(
        Arc::new(forge.clone()),
        Arc::new(clock.clone()),
        hold.clone(),
        Box::new(|_| {}),
    );
    hold.hold(Timestamp(1_000_030));

    assert_eq!(
        unpaced.viewer().unwrap_err(),
        ForgeError::Held(Timestamp(1_000_030))
    );
}

#[test]
fn the_reset_a_used_up_answer_asks_for_takes_its_turn_in_the_pace_too() {
    const CAP: usize = 3;
    let home = tempfile::tempdir().unwrap();
    let clock = FakeClock::at(1_000_000);
    let hold = ForgeHold::shared(Arc::new(LocalPace::with(CallPacer::new(CAP, 3))));
    let forge = FakeForge::new(home.path().join("koji"));
    let ledger = Arc::new(Ledger::in_home(home.path(), "x"));
    let koji = runner(&forge, &clock, &hold, "koji", &ledger);
    koji.viewer().unwrap();
    koji.viewer().unwrap();
    // The last call of the minute is refused with no word of when to ask again.
    forge.set_used_up(None);

    koji.viewer().unwrap_err();

    let lines = forge_ledger::read(&home.path().join(forge_ledger::FILE)).unwrap();
    let mut minutes = std::collections::BTreeMap::<u64, usize>::new();
    for line in &lines {
        *minutes.entry(line.at.0 / 60).or_default() += 1;
    }
    assert_eq!(lines.len(), 4, "three calls and the reset");
    assert!(minutes.values().all(|n| *n <= CAP), "{minutes:?}");
}
