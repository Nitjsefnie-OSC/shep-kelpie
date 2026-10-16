//! The limits kelpie puts on triage, checked before an issue's research call,
//! through the runner's stand-ins
//!
//! None of them calls a model. The day's cap holds an issue in the due list;
//! the author's cap and the account's age end it at `needs-triage` with no call
//! and no comment; the body limit cuts what the call is given. The maintainer's
//! own issues and one `shep kelpie triage` asked for skip all four.

use std::sync::Mutex;

use serde_json::{Value, json};

use super::comment::MARKER;
use super::limits::DAY;
use super::pick_tests::{MAINTAINER, due, issue, list, rig_with, triaging};
use super::research_tests::{WORDS, calls, moves, reply, triage};
use crate::ports::{AuthorAssociation, Clock, OpenIssue, Role, Timestamp};
use crate::runner::{Runner, step};
use crate::state::StateStore;
use crate::test::{Rig, Scripted};

// A rig with `[triage]` set to `lines`, its representative saying `WORDS` and
// its triager answering with nothing found
fn limits_on(lines: &str) -> (Rig, Mutex<Runner>) {
    let rig = rig_with(lines);
    let runner = rig.open().unwrap();
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);
    (rig, runner)
}

// Records saved to the state, as triages that already happened leave them: one
// per `(author, when it was made)`, numbered from 11, none held by a limit.
// The runner must be closed first, as a restart is.
fn seeded(rig: &Rig, records: &[(&str, u64)]) {
    let store = StateStore::new(rig.paths().state);
    let mut saved = serde_json::to_value(store.load().unwrap().unwrap()).unwrap();
    let kept: Vec<Value> = records
        .iter()
        .enumerate()
        .map(|(n, (author, at))| {
            json!({
                "issue": 11 + n as u64,
                "comment": 11 + n as u64,
                "step": "done",
                "at": at,
                "author": author,
            })
        })
        .collect();
    saved["triage"]["issues"] = json!(kept);
    let state = serde_json::from_value(saved).unwrap();
    store.save(&state).unwrap();
}

// The record `status` gives issue `number`, or none
fn record(rig: &Rig, runner: &Mutex<Runner>, number: u64) -> Value {
    let issues = triage(rig, runner)["issues"].clone();
    let found = (issues.as_array().unwrap_or(&Vec::new()))
        .iter()
        .find(|r| r["issue"] == json!(number))
        .cloned();
    found.unwrap_or(Value::Null)
}

// What the triager was asked, which holds the issue fenced as data
fn asked(rig: &Rig) -> String {
    let mut seen = calls(rig, Role::Triager);
    assert_eq!(seen.len(), 1, "one research call");
    seen.remove(0).call.prompt
}

// An issue by an author whose account the forge counts as no history at all
fn fresh(rig: &Rig, number: u64, author: &str, after: u64) -> OpenIssue {
    let mut fresh = issue(rig, number, author, after);
    fresh.author_association = Some(AuthorAssociation::None);
    fresh
}

// The issues kelpie put `label` on, in the order it did
fn labelled(rig: &Rig, label: &str) -> Vec<u64> {
    (rig.github.writes())
        .into_iter()
        .filter(|(_, call)| call.path.ends_with("/labels"))
        .filter_map(|(_, call)| {
            let body: Value = serde_json::from_str(call.body.as_deref()?).ok()?;
            if body["labels"][0].as_str()? != label {
                return None;
            }
            let parts: Vec<&str> = call.path.split('/').collect();
            let at = (parts.iter()).position(|p| *p == "issues")?;
            parts.get(at + 1)?.parse().ok()
        })
        .collect()
}

#[test]
fn the_eleventh_issue_in_a_day_waits_and_is_triaged_once_a_place_opens() {
    let (rig, runner) = triaging("all");
    let now = rig.clock.now().0;
    drop(runner);
    // The day's ten calls, the oldest made now
    seeded(&rig, &vec![("someone", now); 10]);
    let runner = rig.open().unwrap();
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);
    list(&rig, issue(&rig, 30, "newcomer", 5));

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(calls(&rig, Role::Triager).len(), 0, "no eleventh call");
    assert_eq!(rig.github.comments(), [], "nothing is posted");
    assert_eq!(due(&rig, &runner), [30], "it waits in the due list");
    let daily = triage(&rig, &runner)["daily"].clone();
    assert_eq!(daily["used"], 10);
    assert_eq!(daily["cap"], 10);
    assert_eq!(daily["opens"], now + DAY, "when the next place opens");
    assert_eq!(daily["wait"], DAY, "and how long from now");
    assert_eq!(record(&rig, &runner, 30), Value::Null, "no record");

    // A place opens when the oldest of the day's calls is a day old, and the
    // issue that was waiting takes it.
    rig.clock.advance(DAY + 1);
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(calls(&rig, Role::Triager).len(), 1);
    assert_eq!(
        rig.github.comments(),
        [(30, format!("{WORDS}\n\n{MARKER}"))]
    );
    assert_eq!(triage(&rig, &runner)["daily"]["used"], 1);
}

#[test]
fn an_authors_third_issue_in_a_day_gets_needs_triage_with_no_call_and_no_comment() {
    let (rig, runner) = triaging("all");
    let now = rig.clock.now().0;
    drop(runner);
    // Two calls already spent on this author's issues, under two spellings of
    // the login, and one on someone else's
    seeded(
        &rig,
        &[("Newcomer", now), ("newcomer", now), ("someone", now)],
    );
    let runner = rig.open().unwrap();
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);
    list(&rig, issue(&rig, 30, "newcomer", 5));
    list(&rig, issue(&rig, 31, "someone", 6));

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        calls(&rig, Role::Triager).len(),
        1,
        "only the other author's issue is called"
    );
    assert_eq!(
        rig.github.comments(),
        [(31, format!("{WORDS}\n\n{MARKER}"))],
        "no comment on the held issue"
    );
    assert_eq!(
        labelled(&rig, "needs-triage"),
        [30, 31],
        "the held issue is labelled, and nothing else is written on it"
    );
    let held = record(&rig, &runner, 30);
    assert_eq!(held["step"], "done");
    assert_eq!(held["comment"], 0, "no comment was posted");
    assert_eq!(held["held"], "author-cap");
    assert_eq!(held["author"], "newcomer");
    assert_eq!(due(&rig, &runner), Vec::<u64>::new(), "the pass is over it");

    // It stays out of the pass, whatever the next poll lists.
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(calls(&rig, Role::Triager).len(), 1);
    assert_eq!(rig.github.comments().len(), 1);
}

#[test]
fn a_new_account_with_no_history_gets_needs_triage_with_no_call() {
    let (rig, runner) = triaging("all");
    let now = rig.clock.now().0;
    rig.forge
        .set_account_created("newcomer", Timestamp(now - 5 * DAY));
    list(&rig, fresh(&rig, 30, "newcomer", 5));

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(calls(&rig, Role::Triager).len(), 0);
    assert_eq!(rig.github.comments(), []);
    assert_eq!(moves(&rig), ["make needs-triage", "label needs-triage"]);
    let held = record(&rig, &runner, 30);
    assert_eq!(held["held"], "new-account");
    assert_eq!(held["comment"], 0);
    assert_eq!(held["step"], "done");
    assert_eq!(due(&rig, &runner), Vec::<u64>::new());
}

#[test]
fn a_new_account_that_has_contributed_before_is_triaged() {
    let (rig, runner) = triaging("all");
    let now = rig.clock.now().0;
    // Both accounts are five days old: only the one with no history is held
    rig.forge
        .set_account_created("newcomer", Timestamp(now - 5 * DAY));
    rig.forge
        .set_account_created("regular", Timestamp(now - 5 * DAY));
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);
    list(&rig, fresh(&rig, 30, "newcomer", 5));
    list(&rig, issue(&rig, 31, "regular", 6));

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(calls(&rig, Role::Triager).len(), 1);
    assert_eq!(
        rig.github.comments(),
        [(31, format!("{WORDS}\n\n{MARKER}"))]
    );
    assert_eq!(record(&rig, &runner, 30)["held"], "new-account");
    assert_eq!(record(&rig, &runner, 31).get("held"), None);

    // The held one is triaged once the account is old enough
    rig.clock.advance(30 * DAY);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(calls(&rig, Role::Triager).len(), 1, "it was labelled over");
}

#[test]
fn a_long_body_reaches_the_call_cut_with_a_note_and_the_title_whole() {
    let rig = rig_with("pick = \"all\"");
    let runner = rig.open().unwrap();
    let mut crash = issue(&rig, 30, "newcomer", 5);
    crash.title = "Crash on start".to_owned();
    crash.body = format!(
        "HEAD-OF-BODY\n{}",
        "zz ".repeat(10_000) // 30 KB of it, and the tail past the cut
    );
    crash.body.push_str("END-OF-BODY");
    list(&rig, crash.clone());
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    let prompt = asked(&rig);
    assert!(prompt.contains("# Crash on start"), "the title is whole");
    assert!(
        prompt.contains(&crash.body[..20 * 1024]),
        "the body is cut at 20 KB"
    );
    assert!(
        !prompt.contains(&crash.body[20 * 1024..]),
        "nothing past the cut is given"
    );
    assert!(
        prompt.contains("it is 30 KB"),
        "the note says how long it was"
    );
    assert!(
        prompt.contains("at most 20 KB"),
        "and the size it was cut to"
    );
}

#[test]
fn a_cut_body_never_ends_mid_character() {
    let rig = rig_with("pick = \"all\"");
    let runner = rig.open().unwrap();
    let mut crash = issue(&rig, 30, "newcomer", 5);
    crash.title = "Crash on start".to_owned();
    // The 20 KB boundary falls inside the two bytes of `é`
    crash.body = format!("HEAD-OF-BODY{}éEND-OF-BODY", "z".repeat(20_467));
    list(&rig, crash.clone());
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    let prompt = asked(&rig);
    assert!(
        prompt.contains(&crash.body[..20 * 1024 - 1]),
        "the cut steps back to the character before the boundary"
    );
    assert!(!prompt.contains("é"), "no half of a character is given");
    assert!(!prompt.contains("END-OF-BODY"));
}

#[test]
fn the_maintainers_issues_and_the_verb_skip_every_limit() {
    // The maintainer's own issue, with every limit already spent on it
    let rig = rig_with("pick = \"all\"");
    let runner = rig.open().unwrap();
    let now = rig.clock.now().0;
    rig.forge
        .set_account_created(MAINTAINER, Timestamp(now - DAY));
    drop(runner);
    seeded(
        &rig,
        &[("someone", now)]
            .repeat(8)
            .into_iter()
            .chain([("Boss", now), ("BOSS", now)])
            .collect::<Vec<_>>(),
    );
    let runner = rig.open().unwrap();
    let mut own = fresh(&rig, 30, MAINTAINER, 5);
    own.body = "HEAD-OF-BODY".to_owned() + &"zz ".repeat(10_000);
    own.body.push_str("END-OF-BODY");
    list(&rig, own.clone());
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        calls(&rig, Role::Triager).len(),
        1,
        "the day's cap is spent and it is triaged anyway"
    );
    assert_eq!(
        rig.github.comments(),
        [(30, format!("{WORDS}\n\n{MARKER}"))]
    );
    let prompt = asked(&rig);
    assert!(prompt.contains("END-OF-BODY"), "its body is not cut");
    assert_eq!(record(&rig, &runner, 30).get("held"), None);

    // An issue the verb asked for, from a new account with no history, on a day
    // whose calls are all spent
    let rig = rig_with("pick = \"all\"");
    let runner = rig.open().unwrap();
    let now = rig.clock.now().0;
    rig.forge
        .set_account_created("newcomer", Timestamp(now - DAY));
    drop(runner);
    seeded(&rig, &vec![("someone", now); 10]);
    let runner = rig.open().unwrap();
    list(&rig, fresh(&rig, 30, "newcomer", 5));
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    let reply = rig.ask(&runner, "triage", Some("30"));
    assert_eq!(reply.get("error"), None, "{reply}");
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(calls(&rig, Role::Triager).len(), 1);
    assert_eq!(
        rig.github.comments(),
        [(30, format!("{WORDS}\n\n{MARKER}"))]
    );
    assert_eq!(record(&rig, &runner, 30).get("held"), None);
}

#[test]
fn zero_turns_each_limit_off() {
    let rig = rig_with(
        "pick = \"all\"\nper_day = 0\nper_author_per_day = 0\nmin_account_days = 0\nmax_body_kb = 0",
    );
    let runner = rig.open().unwrap();
    let now = rig.clock.now().0;
    rig.forge
        .set_account_created("newcomer", Timestamp(now - 10));
    drop(runner);
    // What each limit would have held it for, on a day full of calls by its own
    // author and by others
    seeded(
        &rig,
        &[("newcomer", now)]
            .repeat(2)
            .into_iter()
            .chain(vec![("someone", now); 8])
            .collect::<Vec<_>>(),
    );
    let runner = rig.open().unwrap();
    for number in [30, 31, 32] {
        let mut long = fresh(&rig, number, "newcomer", 5 + number);
        long.body = "HEAD-OF-BODY".to_owned() + &"zz ".repeat(10_000);
        long.body.push_str("END-OF-BODY");
        list(&rig, long);
    }
    rig.claude
        .script_for(Role::Representative, (0..3).map(|_| Scripted::Say(WORDS)));
    rig.claude.script_for(
        Role::Triager,
        [reply(json!({})), reply(json!({})), reply(json!({}))],
    );

    for _ in 0..3 {
        assert_eq!(step(&runner).unwrap(), None);
        rig.next_look();
    }

    assert_eq!(calls(&rig, Role::Triager).len(), 3, "all three are called");
    let prompts = (calls(&rig, Role::Triager))
        .into_iter()
        .map(|seen| seen.call.prompt)
        .collect::<Vec<_>>();
    assert!(
        prompts.iter().all(|p| p.contains("END-OF-BODY")),
        "no body is cut"
    );
    assert_eq!(rig.github.comments().len(), 3, "and each gets its comment");
    assert_eq!(
        triage(&rig, &runner).get("daily"),
        None,
        "no daily cap to show"
    );
}

#[test]
fn an_accounts_age_is_read_once_per_author() {
    let (rig, runner) = limits_on("pick = \"all\"\nper_author_per_day = 0");
    let now = rig.clock.now().0;
    for login in ["newcomer", "other"] {
        rig.forge.set_account_created(login, Timestamp(now - 3));
    }
    for (number, author) in [
        (30, "newcomer"),
        (31, "Newcomer"),
        (32, "NEWCOMER"),
        (33, "other"),
    ] {
        list(&rig, fresh(&rig, number, author, 5 + number));
    }

    for _ in 0..4 {
        assert_eq!(step(&runner).unwrap(), None);
        rig.next_look();
    }

    assert_eq!(calls(&rig, Role::Triager).len(), 0, "all four are held");
    assert_eq!(
        rig.forge.account_reads("newcomer"),
        1,
        "once for three issues"
    );
    assert_eq!(rig.forge.account_reads("other"), 1);

    // An age the forge cannot say holds nothing, and is not asked again
    rig.forge.set_accounts_down(true);
    list(&rig, fresh(&rig, 34, "another", 60));
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(
        calls(&rig, Role::Triager).len(),
        1,
        "an unreadable age holds nothing"
    );
    assert_eq!(rig.forge.account_reads("another"), 1);
}
