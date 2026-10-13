use std::sync::Mutex;

use serde_json::{Value, json};

use super::tests::{opened, picking, poll};
use crate::ports::{Clock, ForgeError};
use crate::runner::{Runner, step};
use crate::test::{Rig, VIEWER};

// The contributor's pull request, once the runner follows it
fn followed_30() -> (Rig, Mutex<Runner>, String) {
    let (rig, runner) = picking("all");
    let head = opened(&rig, 30, "newcomer");
    assert_eq!(step(&runner).unwrap(), None);
    rig.clock.advance(10);
    (rig, runner, head)
}

// What `status` lists under the contribution's `key`
fn listed(rig: &Rig, runner: &Mutex<Runner>, key: &str) -> Vec<Value> {
    let status = rig.ask(runner, "status", None);
    let kept = status["contributions"][0][key].as_array().cloned();
    kept.unwrap_or_default()
}

fn now(rig: &Rig) -> u64 {
    rig.clock.now().0
}

#[test]
fn a_contributors_comment_on_their_pull_request_is_heard() {
    let (rig, runner, _) = followed_30();
    let id = rig.writes_on(30, "newcomer", "Ready for another look");
    let at = now(&rig);

    poll(&rig, &runner);

    assert_eq!(
        listed(&rig, &runner, "heard"),
        [json!({"kind": "comment", "id": id, "at": at})]
    );
    assert_eq!(listed(&rig, &runner, "context"), Vec::<Value>::new());
    assert_eq!(rig.forge.comments(), [], "nothing is said back");
}

#[test]
fn a_contributors_reply_in_a_review_thread_is_heard() {
    let (rig, runner, _) = followed_30();
    let at = rig.clock.now();
    let id = rig
        .forge
        .threads
        .writes_in_review(30, "NewComer", "Fixed", at);

    poll(&rig, &runner);

    assert_eq!(
        listed(&rig, &runner, "heard"),
        [json!({"kind": "reply", "id": id, "at": at.0})]
    );
    assert_eq!(rig.forge.threads.review_reads().len(), 1);
    assert_eq!(rig.forge.comments(), []);
}

#[test]
fn a_push_is_heard_as_a_new_head() {
    let (rig, runner, first) = followed_30();
    assert_eq!(listed(&rig, &runner, "heard"), Vec::<Value>::new());
    let second = rig.push_by_hand("topic/30", "more.txt");
    assert_ne!(first, second);

    poll(&rig, &runner);

    assert_eq!(
        listed(&rig, &runner, "heard"),
        [json!({"kind": "push", "head": second, "at": now(&rig)})]
    );
}

#[test]
fn a_comment_heard_before_a_push_is_still_heard_after_it() {
    let (rig, runner, _) = followed_30();
    let id = rig.writes_on(30, "newcomer", "First");
    poll(&rig, &runner);
    let second = rig.push_by_hand("topic/30", "more.txt");

    poll(&rig, &runner);

    let heard = listed(&rig, &runner, "heard");
    assert_eq!(heard.len(), 2, "{heard:?}");
    assert_eq!(heard[0]["id"], id);
    assert_eq!(heard[1]["head"], second.as_str());
}

#[test]
fn the_maintainers_comment_is_kept_as_context_apart_from_the_contributors() {
    let (rig, runner, _) = followed_30();
    let said = rig.writes_on(30, "boss", "Please add a test");
    let at = rig.clock.now();
    let replied = rig
        .forge
        .threads
        .writes_in_review(30, "Boss", "Here too", at);
    let theirs = rig.writes_on(30, "newcomer", "Done");

    poll(&rig, &runner);

    assert_eq!(
        listed(&rig, &runner, "context"),
        [
            json!({"kind": "comment", "id": said, "at": at.0}),
            json!({"kind": "reply", "id": replied, "at": at.0}),
        ]
    );
    assert_eq!(
        listed(&rig, &runner, "heard"),
        [json!({"kind": "comment", "id": theirs, "at": at.0})]
    );
}

#[test]
fn a_third_partys_a_bots_and_kelpies_own_comment_are_not_heard() {
    let (rig, runner, _) = followed_30();
    let at = rig.clock.now();
    rig.writes_on(30, "stranger", "Nice");
    rig.forge
        .threads
        .bot_writes(30, "coderabbitai", "Review", at);
    // A bot sharing the contributor's login is no more heard than any bot
    rig.forge.threads.bot_writes(30, "newcomer", "Auto", at);
    rig.writes_on(30, VIEWER, "Kelpie, as the login gh acts as");
    rig.forge.threads.writes_in_review(30, "stranger", "Hm", at);
    rig.forge
        .threads
        .bot_writes_in_review(30, "newcomer", "Auto", at);
    rig.forge.threads.writes_in_review(30, VIEWER, "Kelpie", at);

    poll(&rig, &runner);

    assert_eq!(listed(&rig, &runner, "heard"), Vec::<Value>::new());
    assert_eq!(listed(&rig, &runner, "context"), Vec::<Value>::new());
}

#[test]
fn a_comment_kelpie_posted_as_the_maintainers_login_is_no_context() {
    let (rig, runner, _) = followed_30();
    let mine = rig.writes_on(30, "Boss", "Posted by kelpie through gh");
    runner.lock().unwrap().state.posted.push(mine);
    let theirs = rig.writes_on(30, "Boss", "Typed by hand");

    poll(&rig, &runner);

    let context = listed(&rig, &runner, "context");
    assert_eq!(context.len(), 1, "{context:?}");
    assert_eq!(context[0]["id"], theirs);
}

#[test]
fn a_comment_is_heard_once_across_polls() {
    let (rig, runner, _) = followed_30();
    let comment = rig.writes_on(30, "newcomer", "Once");
    let reply = rig
        .forge
        .threads
        .writes_in_review(30, "newcomer", "Once", rig.clock.now());
    let mut seen = Vec::new();

    // Each poll reaches back over the slack, so it reads the comment again.
    for _ in 0..4 {
        poll(&rig, &runner);
        seen.push(listed(&rig, &runner, "heard"));
    }

    let ids: Vec<_> = seen[3].iter().map(|s| s["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, [comment, reply]);
    assert!(seen.iter().all(|heard| heard.len() == 2), "{seen:?}");
    assert_eq!(rig.forge.threads.reads().len(), 4);
}

#[test]
fn one_comments_since_read_serves_a_ruling_and_a_contribution_in_the_same_poll() {
    let (rig, runner, _) = Rig::parked_set("shep", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.contributions]\npick = \"all\"\n"));
    });
    rig.forge.threads.grant_write("octocat");
    opened(&rig, 30, "newcomer");
    rig.clock.advance(5);
    // The first pass alerts the maintainer of the ruling, the next follows
    // the contribution
    rig.next_look();
    step(&runner).unwrap();
    step(&runner).unwrap();
    rig.clock.advance(10);
    let comment = rig.writes_on(30, "newcomer", "Hello");
    let answer = rig.writes_on(71, "octocat", "71 yes");
    let before = rig.forge.threads.reads().len();

    rig.next_look();
    step(&runner).unwrap();

    assert_eq!(rig.forge.threads.reads().len(), before + 1, "one read");
    assert_eq!(
        rig.github.reactions(),
        [answer],
        "the ruling heard its reply"
    );
    let status = rig.ask(&runner, "status", None);
    let heard = &status["contributions"][0]["heard"];
    assert_eq!(
        heard[0]["id"], comment,
        "the contribution heard its comment"
    );
}

#[test]
fn a_contribution_alone_is_read_for_without_a_ruling_and_a_project_picking_nothing_reads_nothing() {
    let (rig, runner, _) = followed_30();
    poll(&rig, &runner);
    assert_eq!(rig.forge.threads.reads().len(), 1);

    let rig = Rig::new("shep");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.next_look();
    step(&runner).unwrap();
    rig.next_look();
    step(&runner).unwrap();
    assert_eq!(rig.forge.threads.reads(), []);
    assert_eq!(rig.forge.threads.review_reads(), []);
}

#[test]
fn a_comment_on_a_pull_request_nobody_follows_is_not_recorded() {
    let (rig, runner, _) = followed_30();
    rig.writes_on(99, "newcomer", "Elsewhere");

    poll(&rig, &runner);

    assert_eq!(listed(&rig, &runner, "heard"), Vec::<Value>::new());
}

#[test]
fn the_record_keeps_no_comment_text() {
    let (rig, runner, _) = followed_30();
    let at = rig.clock.now();
    rig.writes_on(30, "newcomer", "SECRET-WORDS-OF-THE-CONTRIBUTOR");
    rig.writes_on(30, "boss", "SECRET-WORDS-OF-THE-MAINTAINER");
    rig.forge
        .threads
        .writes_in_review(30, "newcomer", "SECRET-WORDS-IN-A-REVIEW", at);

    poll(&rig, &runner);

    let status = rig.ask(&runner, "status", None);
    let contribution = &status["contributions"][0];
    for key in ["heard", "context"] {
        for said in contribution[key].as_array().unwrap() {
            let mut keys: Vec<_> = said.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            assert_eq!(keys, ["at", "id", "kind"], "{said}");
        }
    }
    assert!(
        !contribution.to_string().contains("SECRET"),
        "{contribution}"
    );
    let runner = runner.lock().unwrap();
    let saved = serde_json::to_string(&runner.state).unwrap();
    assert!(!saved.contains("SECRET"), "the saved state keeps no text");
}

#[test]
fn the_record_keeps_the_last_fifty_things_said() {
    let (rig, runner, _) = followed_30();
    let ids: Vec<u64> = (0..60)
        .map(|n| rig.writes_on(30, "newcomer", &format!("comment {n}")))
        .collect();

    poll(&rig, &runner);

    let heard: Vec<u64> = listed(&rig, &runner, "heard")
        .iter()
        .map(|s| s["id"].as_u64().unwrap())
        .collect();
    assert_eq!(heard, ids[10..]);
}

#[test]
fn a_reply_that_dropped_off_the_record_is_not_heard_again_within_the_slack() {
    let (rig, runner, _) = followed_30();
    let ids: Vec<u64> = (0..51)
        .map(|n| {
            rig.clock.advance(1);
            let at = rig.clock.now();
            (rig.forge.threads).writes_in_review(30, "newcomer", &format!("reply {n}"), at)
        })
        .collect();
    poll(&rig, &runner);
    let second = rig.push_by_hand("topic/30", "more.txt");

    // Review threads are read again from the slack behind the last read, so
    // the first replies are asked for once more after the push is heard
    poll(&rig, &runner);
    poll(&rig, &runner);

    let heard = listed(&rig, &runner, "heard");
    let kept: Vec<_> = heard.iter().map(|s| s["id"].as_u64()).collect();
    assert_eq!(heard.len(), 50, "{heard:?}");
    assert_eq!(
        kept[..49],
        ids[2..].iter().map(|i| Some(*i)).collect::<Vec<_>>()
    );
    assert_eq!(heard[49]["head"], second.as_str());
}

#[test]
fn a_review_read_that_fails_holds_back_the_place_read_to_so_the_reply_is_heard_later() {
    let (rig, runner, _) = followed_30();
    let first = rig.clock.now();
    rig.forge
        .threads
        .writes_in_review(30, "newcomer", "Late", first);
    rig.forge.set_thread_replies_down(true);

    poll(&rig, &runner);
    assert_eq!(listed(&rig, &runner, "heard"), Vec::<Value>::new());

    // Past the slack, a reply the failed read missed is still asked for
    rig.clock.advance(20 * 60);
    rig.forge.set_thread_replies_down(false);
    poll(&rig, &runner);

    let heard = listed(&rig, &runner, "heard");
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0]["kind"], "reply");
    let since = rig.forge.threads.review_reads().last().unwrap().1;
    assert!(since <= first, "{since:?} reaches back to {first:?}");
}

#[test]
fn what_was_heard_survives_a_restart_and_is_not_heard_again() {
    let (rig, runner, _) = followed_30();
    let id = rig.writes_on(30, "newcomer", "Before the restart");
    poll(&rig, &runner);
    drop(runner);

    let runner = rig.open().unwrap();
    rig.next_look();
    step(&runner).unwrap();
    rig.next_look();
    step(&runner).unwrap();

    let heard = listed(&rig, &runner, "heard");
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0]["id"], id);
}

#[test]
fn a_review_read_that_failed_is_read_again_from_where_it_began_after_a_restart() {
    let (rig, runner, _) = followed_30();
    let first = rig.clock.now();
    let reply = rig
        .forge
        .threads
        .writes_in_review(30, "newcomer", "Late", first);
    rig.forge.set_thread_replies_down(true);
    poll(&rig, &runner);
    // Something heard later moves the repo's comments' place on
    rig.clock.advance(60 * 60);
    rig.writes_on(30, "newcomer", "Much later");
    poll(&rig, &runner);
    drop(runner);

    rig.forge.set_thread_replies_down(false);
    let runner = rig.open().unwrap();
    rig.next_look();
    step(&runner).unwrap();
    rig.next_look();
    step(&runner).unwrap();

    let heard = listed(&rig, &runner, "heard");
    let ids: Vec<_> = heard.iter().filter_map(|s| s["id"].as_u64()).collect();
    assert!(ids.contains(&reply), "{heard:?}");
}

#[test]
fn nothing_is_recorded_while_the_logins_cannot_be_had_and_all_of_it_is_heard_after() {
    let (rig, runner, _) = followed_30();
    let said = rig.writes_on(30, "boss", "Please add a test");
    rig.clock.advance(5);
    let theirs = rig.writes_on(30, "newcomer", "Done");
    {
        let mut runner = runner.lock().unwrap();
        runner.viewer = None;
    }
    rig.forge
        .set_viewer_down(ForgeError::Failed("no network".into()));
    poll(&rig, &runner);
    assert_eq!(listed(&rig, &runner, "heard"), Vec::<Value>::new());
    assert_eq!(listed(&rig, &runner, "context"), Vec::<Value>::new());
    rig.forge
        .threads
        .write(30, "newcomer", "x", rig.clock.now());
    rig.forge.set_viewer_up();

    poll(&rig, &runner);

    let ids = |key: &str| -> Vec<u64> {
        let kept = listed(&rig, &runner, key);
        kept.iter().map(|s| s["id"].as_u64().unwrap()).collect()
    };
    assert_eq!(ids("context"), [said]);
    assert_eq!(ids("heard")[0], theirs);
}

#[test]
fn the_maintainers_login_in_a_review_thread_is_kelpies_own_reply_and_is_left_alone() {
    // `gh` acts as the maintainer here, so a reply through it is not told
    // from one the maintainer typed
    let rig = Rig::new("shep");
    rig.with_app();
    rig.maintainer(VIEWER);
    rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.contributions]\npick = \"all\"\n"));
    let runner = rig.open().unwrap();
    opened(&rig, 30, "newcomer");
    assert_eq!(step(&runner).unwrap(), None);
    rig.clock.advance(10);
    let at = rig.clock.now();
    rig.forge
        .threads
        .writes_in_review(30, VIEWER, "Posted through gh", at);
    let typed = rig.writes_on(30, VIEWER, "A comment");

    poll(&rig, &runner);

    let context = listed(&rig, &runner, "context");
    assert_eq!(context.len(), 1, "{context:?}");
    assert_eq!(
        context[0],
        json!({"kind": "comment", "id": typed, "at": at.0})
    );
}
