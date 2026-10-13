//! Files saved by older versions of kelpie, loaded and saved again

use std::fs;

use super::super::*;
use super::store_in;
use crate::test::{a_work_item, a_work_item_from_before_comments};

#[test]
fn a_version_6_file_loads_with_nothing_attached_and_saves_at_the_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut state = ProjectState::new();
    state.work_items.push(a_work_item());
    let mut old = serde_json::to_value(&state).unwrap();
    old["version"] = 6.into();
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.work_items[0].attached, None);
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
}

#[test]
fn a_version_7_file_loads_with_no_project_manager_and_saves_at_the_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 7.into();
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(
        (&loaded.pm_session, &loaded.pm_told, &loaded.pm_attached),
        (&None, &Vec::new(), &None)
    );
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
}

#[test]
fn a_version_9_file_saved_paused_loads_and_saves_with_no_run_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 9.into();
    old["run"] = "paused".into();
    old["since"] = 7.into();
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded, ProjectState::new());
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!((saved.get("run"), saved.get("since")), (None, None));
}

#[test]
fn a_version_12_file_loads_and_saves_at_the_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut state = ProjectState::new();
    state.work_items.push(a_work_item_from_before_comments());
    let mut old = serde_json::to_value(&state).unwrap();
    old["version"] = 12.into();
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded, state);
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
}

// A version 14 file has no `late_from` on its work items and no `late_fix`
// or `nits` on its merge rulings, which load as their defaults.
#[test]
fn a_version_14_file_loads_with_no_late_fix_and_saves_at_the_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 14.into();
    let mut item = serde_json::to_value(a_work_item()).unwrap();
    item["noted_from"] = "c0ffee".into();
    old["work_items"] = serde_json::json!([item]);
    old["rulings"] = serde_json::json!([{
        "id": 1,
        "issue": 42,
        "question": "q",
        "pull_request": 51,
        "kind": { "kind": "merge", "head": "f1x", "note_fix": true },
        "alerted": true,
    }]);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    let item = &loaded.work_items[0];
    assert_eq!(
        (item.noted_from.as_deref(), item.late_from.as_deref()),
        (Some("c0ffee"), None)
    );
    assert_eq!(
        loaded.rulings[0].kind,
        RulingKind::Merge {
            head: "f1x".into(),
            unreviewed: None,
            open_threads: None,
            held_by: Vec::new(),
            unread_head: false,
            note_fix: true,
            late_fix: false,
            nits: None,
        }
    );
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
}

// A version 15 file has no `ci_fix_turns` in a work item's counts and no
// `fix_turns` on a still-red ruling: none counted, and the pushed-no-fix ruling.
#[test]
fn a_version_15_file_loads_and_saves_at_the_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 15.into();
    let mut item = serde_json::to_value(a_work_item()).unwrap();
    item["counts"] = serde_json::json!({ "review_rounds": 2, "fix_turns": 1, "rulings": 1 });
    old["work_items"] = serde_json::json!([item]);
    let still_red = serde_json::json!({
        "kind": "stuck",
        "reason": "still-red",
        "head": "bad",
        "checks": ["lint"],
    });
    old["rulings"] = serde_json::json!([{
        "id": 1,
        "issue": 42,
        "question": "q",
        "pull_request": 51,
        "kind": still_red,
        "alerted": true,
    }]);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    let counts = loaded.work_items[0].counts;
    assert_eq!((counts.fix_turns, counts.ci_fix_turns), (1, 0));
    assert_eq!(
        loaded.rulings[0].kind,
        RulingKind::from(Stuck::StillRed {
            head: "bad".into(),
            checks: vec!["lint".into()],
            fix_turns: None,
            review: None,
        })
    );
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(saved["work_items"][0]["counts"], item["counts"]);
    assert_eq!(saved["rulings"][0]["kind"], still_red);
}

// A version 20 file has no `thread_answers`: a fix turn under way when it was
// saved has said nothing of its threads, so each is replied to as not addressed.
// The worker's lines are pinned as they are saved.
#[test]
fn a_version_20_file_loads_with_no_thread_answers_and_saves_them_pinned() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 20.into();
    let mut item = serde_json::to_value(a_work_item()).unwrap();
    item["threads_sent"] = serde_json::json!(["PRRT_a", "PRRT_b"]);
    old["work_items"] = serde_json::json!([item]);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.work_items[0].thread_answers, []);
    loaded.work_items[0].answer_threads("thread 1: fixed\nthread 2: declined: deliberate");
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(
        saved["work_items"][0]["thread_answers"],
        serde_json::json!([
            { "said": "fixed", "thread": "PRRT_a" },
            { "said": "declined", "thread": "PRRT_b", "reason": "deliberate" },
        ])
    );
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// A version 29 file has no `verdict`: its rulings were voted on with nothing
// concluded, and a verdict saved since is kept as the panel left it.
#[test]
fn a_version_29_file_loads_with_no_verdicts_and_a_verdict_is_kept() {
    use crate::panel::{Advice, Tier, Verdict};

    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut state = ProjectState::new();
    state.rulings.push(Ruling {
        id: 1,
        issue: Some(7),
        question: "q".into(),
        pull_request: Some(71),
        kind: RulingKind::Stuck(Stuck::Closed),
        alerted: true,
        panel: None,
        verdict: None,
        unposted: false,
        skipped: None,
        effects: Vec::new(),
        raised: None,
    });
    let mut old = serde_json::to_value(&state).unwrap();
    old["version"] = 29.into();
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.rulings[0].verdict, None);
    loaded.rulings[0].verdict = Some(Verdict {
        work: "fix".into(),
        tier: Tier::Fix,
        advice: Some(Advice {
            choice: 'A',
            agree: 2,
            of: 2,
        }),
        risky: false,
        shadow: false,
    });
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(
        saved["rulings"][0]["verdict"],
        serde_json::json!({
            "work": "fix", "tier": "fix",
            "advice": { "choice": "A", "agree": 2, "of": 2 },
        })
    );
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// A ruling saved before `raised` loads with no time, and one raised since
// keeps its time through a save and a load.
#[test]
fn a_ruling_saved_without_raised_loads_and_a_raised_time_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut state = ProjectState::new();
    state.rulings.push(Ruling {
        id: 1,
        issue: Some(7),
        question: "q".into(),
        pull_request: Some(71),
        kind: RulingKind::Stuck(Stuck::Closed),
        alerted: true,
        panel: None,
        verdict: None,
        unposted: false,
        skipped: None,
        effects: Vec::new(),
        raised: None,
    });
    let old = serde_json::to_value(&state).unwrap();
    assert!(old["rulings"][0].get("raised").is_none());
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.rulings[0].raised, None);
    loaded.rulings[0].raised = Some(Timestamp(1_700_000_000));
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["rulings"][0]["raised"], 1_700_000_000);
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// A version 30 file has no `order`, so it loads with the board following its
// rule, and an order a later read keeps saves and loads back.
#[test]
fn a_version_30_file_loads_with_an_empty_order_and_saves_at_the_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 30.into();
    old.as_object_mut().unwrap().remove("order");
    old.as_object_mut().unwrap().remove("ranked");
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.order, Vec::<u64>::new());
    assert!(loaded.ranked.is_empty());
    loaded.order = vec![7, 3, 9];
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        (&saved["version"], &saved["order"]),
        (&serde_json::json!(VERSION), &serde_json::json!([7, 3, 9]))
    );
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// A version 33 file has no `test` on a finding: it loads with none attached, a
// finding with one keeps it through a save, and one without saves no key.
#[test]
fn a_state_file_without_a_test_still_loads() {
    use crate::ports::Severity;
    use crate::work_item::{Phase, Review, ReviewStage};

    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let state_with = |test: Option<&str>| {
        let mut item = a_work_item_from_before_comments();
        item.phase = Phase::Review(Review {
            stage: ReviewStage::Found {
                findings: vec![Finding {
                    severity: Severity::High,
                    file: "src/lib.rs".into(),
                    line: 3,
                    what: "it panics".into(),
                    why: "a caller sees it".into(),
                    test: test.map(str::to_owned),
                }],
                threads: Vec::new(),
            },
            ..Review::first()
        });
        let mut state = ProjectState::new();
        state.work_items.push(item);
        state
    };
    let finding_of =
        |saved: &serde_json::Value| saved["work_items"][0]["phase"]["stage"]["findings"][0].clone();

    let mut old = serde_json::to_value(state_with(None)).unwrap();
    old["version"] = 33.into();
    assert!(finding_of(&old).get("test").is_none(), "{old}");
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();
    assert_eq!(store.load().unwrap(), Some(state_with(None)));

    let attached = state_with(Some("tests/panics.rs"));
    store.save(&attached).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(finding_of(&saved)["test"], "tests/panics.rs");
    assert_eq!(store.load().unwrap(), Some(attached));
}

// A version 34 file has no `test_checks`: it loads with none, a work item with
// a check keeps it through a save, and one without saves no key.
#[test]
fn a_state_file_without_test_checks_still_loads() {
    use crate::ports::TestOutcome;
    use crate::work_item::TestCheck;

    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let state_with = |checks: Vec<TestCheck>| {
        let mut item = a_work_item_from_before_comments();
        item.test_checks = checks;
        let mut state = ProjectState::new();
        state.work_items.push(item);
        state
    };

    let mut old = serde_json::to_value(state_with(Vec::new())).unwrap();
    old["version"] = 34.into();
    assert!(old["work_items"][0].get("test_checks").is_none(), "{old}");
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();
    assert_eq!(store.load().unwrap(), Some(state_with(Vec::new())));

    let checked = state_with(vec![TestCheck {
        file: "src/lib.rs".into(),
        line: 3,
        what: "it panics".into(),
        round: 2,
        before: TestOutcome::Failed {
            output: "boom".into(),
        },
        after: Some(TestOutcome::Passed),
        sent_back: false,
    }]);
    store.save(&checked).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(
        saved["work_items"][0]["test_checks"],
        serde_json::json!([{
            "file": "src/lib.rs",
            "line": 3,
            "what": "it panics",
            "round": 2,
            "before": { "result": "failed", "output": "boom" },
            "after": { "result": "passed" },
        }])
    );
    assert_eq!(store.load().unwrap(), Some(checked));
}

// A version 35 file has no `reran`: it loads with none, a work item that reran
// a head keeps it through a save, and one that did not saves no key.
#[test]
fn a_state_file_without_reran_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let state_with = |reran: Option<&str>| {
        let mut item = a_work_item_from_before_comments();
        item.reran = reran.map(str::to_owned);
        let mut state = ProjectState::new();
        state.work_items.push(item);
        state
    };

    let mut old = serde_json::to_value(state_with(None)).unwrap();
    old["version"] = 35.into();
    assert!(old["work_items"][0].get("reran").is_none(), "{old}");
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();
    assert_eq!(store.load().unwrap(), Some(state_with(None)));

    let reran = state_with(Some("c0ffee"));
    store.save(&reran).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(saved["work_items"][0]["reran"], "c0ffee");
    assert_eq!(store.load().unwrap(), Some(reran));
}

// A version 38 file has no `talked` on its work items, so no comment is
// handled and no round counted. A work item with a pull request loads with
// its comments marked as seen, since its worker has read them.
#[test]
fn a_version_38_file_loads_with_no_comment_handled_and_saves_at_the_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 38.into();
    let item = serde_json::to_value(a_work_item()).unwrap();
    assert!(item.get("talked").is_none(), "{item}");
    old["work_items"] = serde_json::json!([item]);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    let talked = &loaded.work_items[0].talked;
    assert!(talked.comments.is_empty() && talked.rounds == 0);
    // Its pull request has comments its worker has seen.
    assert_eq!(talked.quiet, loaded.work_items[0].pull_request.is_some());
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
}

// A version 40 file has no `sent_back` on its checks: they load as not sent
// back, and one that was sent back keeps the mark through a save.
#[test]
fn a_state_file_without_the_sent_back_mark_still_loads() {
    use crate::ports::TestOutcome;
    use crate::work_item::TestCheck;

    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let check = |sent_back| TestCheck {
        file: "src/lib.rs".into(),
        line: 3,
        what: "it panics".into(),
        round: 2,
        before: TestOutcome::Failed {
            output: "boom".into(),
        },
        after: Some(TestOutcome::Failed {
            output: "boom".into(),
        }),
        sent_back,
    };
    let state_with = |check: TestCheck| {
        let mut item = a_work_item_from_before_comments();
        item.test_checks = vec![check];
        let mut state = ProjectState::new();
        state.work_items.push(item);
        state
    };

    let mut old = serde_json::to_value(state_with(check(false))).unwrap();
    old["version"] = 40.into();
    assert!(
        old["work_items"][0]["test_checks"][0]
            .get("sent_back")
            .is_none(),
        "{old}"
    );
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();
    assert_eq!(store.load().unwrap(), Some(state_with(check(false))));

    store.save(&state_with(check(true))).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(saved["work_items"][0]["test_checks"][0]["sent_back"], true);
    assert_eq!(store.load().unwrap(), Some(state_with(check(true))));
}

#[test]
fn an_older_state_file_loads_with_no_contributions() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 39.into();
    assert_eq!(old.get("contributions"), None);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.contributions, []);
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
}

#[test]
fn a_followed_contribution_survives_a_save_and_a_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut state = ProjectState::new();
    state.contributions.push(Contribution {
        pull_request: 12,
        author: "newcomer".into(),
        head: "2a8f1e6".into(),
        seen: Timestamp(1_790_000_000),
    });
    store.save(&state).unwrap();
    assert_eq!(store.load().unwrap(), Some(state));
}

// A version 41 file has no `skipped`: its rulings loaded as any panel may
// still vote on them, and a skip saved since is kept as the runner left it.
#[test]
fn a_version_41_file_loads_with_no_skips_and_a_skip_is_kept() {
    use crate::panel::Skip;

    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut state = ProjectState::new();
    state.rulings.push(Ruling {
        id: 71,
        issue: Some(7),
        question: "q".into(),
        pull_request: Some(71),
        kind: RulingKind::Stuck(Stuck::Closed),
        alerted: false,
        panel: None,
        verdict: None,
        unposted: true,
        skipped: None,
        effects: Vec::new(),
        raised: None,
    });
    let mut old = serde_json::to_value(&state).unwrap();
    old["version"] = 41.into();
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.rulings[0].skipped, None);
    loaded.rulings[0].skipped = Some(Skip::Draining);
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(saved["rulings"][0]["skipped"], "draining");
    assert_eq!(store.load().unwrap(), Some(loaded));
}
