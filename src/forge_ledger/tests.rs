use super::*;

// 2026-10-03T04:00:00Z, the start of a minute
const START: u64 = 1_791_000_000;

// Two runners appending to the one ledger, as they do in a kelpie home.
fn recorded(home: &Path) -> Vec<Line> {
    let koji = Ledger::in_home(home, "koji");
    let shep = Ledger::in_home(home, "shep");
    let at = |seconds: u64| Timestamp(START + seconds);
    for seconds in [5, 10, 15] {
        koji.record(at(seconds), Kind::Reviewed, 120, Ended::Ok);
    }
    shep.record(at(20), Kind::PullRequest, 80, Ended::Ok);
    for seconds in [65, 70] {
        koji.record(at(seconds), Kind::OpenPullRequests, 300, Ended::Ok);
    }
    for seconds in [95, 100, 105, 110] {
        koji.record(at(seconds), Kind::Reviewed, 120, Ended::Ok);
    }
    koji.record(at(150), Kind::Reviewed, 90, Ended::UsedUp);
    koji.record(at(155), Kind::Viewer, 0, Ended::Held);
    read(&home.join(FILE)).unwrap()
}

#[test]
fn a_ledger_holds_a_line_for_each_call_and_reads_back_as_written() {
    let home = tempfile::tempdir().unwrap();
    let lines = recorded(home.path());

    assert_eq!(lines.len(), 12);
    assert_eq!(
        lines[3],
        Line {
            at: Timestamp(START + 20),
            project: "shep".to_owned(),
            kind: Kind::PullRequest,
            api: Api::GraphQl,
            by: Identity::Login,
            millis: 80,
            ended: Ended::Ok,
            not_modified: 0,
        }
    );
    let text = std::fs::read_to_string(home.path().join(FILE)).unwrap();
    assert_eq!(
        text.lines().nth(3).unwrap(),
        r#"{"at":1791000020,"project":"shep","kind":"pull_request","api":"graphql","by":"login","millis":80,"ended":"ok"}"#
    );
    assert!(
        text.contains(r#""kind":"viewer","api":"rest","by":"login","millis":0,"ended":"held""#)
    );
    assert!(text.contains(r#""ended":"used-up""#));
}

#[test]
fn a_torn_line_and_a_kind_a_newer_kelpie_wrote_are_passed_over() {
    let home = tempfile::tempdir().unwrap();
    recorded(home.path());
    let path = home.path().join(FILE);
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("{\"at\":1791000200,\"project\":\"koji\",\"kind\":\"a_newer_call\"}\n{\"at\":17");
    std::fs::write(&path, text).unwrap();

    assert_eq!(read(&path).unwrap().len(), 12);
    // The next append ends the torn tail with a newline of its own.
    let ledger = Ledger::in_home(home.path(), "koji");
    ledger.record(Timestamp(START + 300), Kind::Issue, 5, Ended::Ok);
    let lines = read(&path).unwrap();
    assert_eq!(lines.len(), 13);
    assert_eq!(lines[12].kind, Kind::Issue);
    assert!(read(&home.path().join("none.jsonl")).unwrap().is_empty());
}

#[test]
fn the_report_counts_calls_made_in_each_minute_and_leaves_held_ones_out() {
    let home = tempfile::tempdir().unwrap();
    let lines = recorded(home.path());

    let out = report(&lines, None);

    assert_eq!(
        out,
        [
            "forge calls from 2026-10-03T04:00:05Z to 2026-10-03T04:02:35Z: 11 made, 1 held, 1 used up",
            "by project (calls, minutes with a call, a minute, busiest minute):",
            "  koji: 10, 3, 3.3, 6",
            "  shep: 1, 1, 1.0, 1",
            "by identity (calls, minutes with a call, a minute, busiest minute):",
            "  login: 11, 3, 3.7, 6",
            "by kind (calls, minutes with a call, a minute, busiest minute):",
            "  reviewed graphql: 8, 3, 2.7, 4",
            "  open_pull_requests graphql: 2, 1, 2.0, 2",
            "  pull_request graphql: 1, 1, 1.0, 1",
            "busiest minutes:",
            "  2026-10-03T04:01:00Z: 6 calls (koji 6); reviewed 4, open_pull_requests 2",
            "  2026-10-03T04:00:00Z: 4 calls (koji 3, shep 1); reviewed 3, pull_request 1",
            "  2026-10-03T04:02:00Z: 1 call (koji 1); reviewed 1",
            "used up:",
            "  2026-10-03T04:02:30Z koji reviewed (graphql); the 60 seconds before: 4 calls (koji 4); reviewed 4",
        ]
    );
}

#[test]
fn the_report_from_a_date_leaves_out_earlier_calls() {
    let home = tempfile::tempdir().unwrap();
    let lines = recorded(home.path());

    let out = report(&lines, Some(Timestamp(START + 60)));

    assert_eq!(
        out[0],
        "forge calls from 2026-10-03T04:01:05Z to 2026-10-03T04:02:35Z: 7 made, 1 held, 1 used up"
    );
    assert!(out.contains(&"  koji: 7, 2, 3.5, 6".to_owned()), "{out:#?}");
    assert!(!out.iter().any(|line| line.contains("shep")), "{out:#?}");
}

#[test]
fn a_ledger_with_no_used_up_answer_says_so_and_an_empty_one_says_it_is_empty() {
    let home = tempfile::tempdir().unwrap();
    Ledger::in_home(home.path(), "koji").record(Timestamp(START), Kind::Issue, 50, Ended::Ok);
    let lines = read(&home.path().join(FILE)).unwrap();

    let out = report(&lines, None);
    assert_eq!(&out[out.len() - 2..], ["used up:", "  none"]);
    assert_eq!(report(&[], None), ["no forge calls recorded"]);
}

#[test]
fn a_used_up_answer_reads_its_window_from_the_whole_ledger_by_time() {
    let home = tempfile::tempdir().unwrap();
    let koji = Ledger::in_home(home.path(), "koji");
    let shep = Ledger::in_home(home.path(), "shep");
    let at = |seconds: u64| Timestamp(START + seconds);
    koji.record(at(100), Kind::Reviewed, 5, Ended::Ok);
    koji.record(at(150), Kind::Reviewed, 5, Ended::UsedUp);
    // Appended after the used-up answer, though made before it
    shep.record(at(140), Kind::Viewer, 5, Ended::Ok);
    let lines = read(&home.path().join(FILE)).unwrap();

    let last = |out: Vec<String>| out.last().unwrap().clone();
    let tail = "the 60 seconds before: 2 calls (koji 1, shep 1); reviewed 1, viewer 1";
    assert!(last(report(&lines, None)).ends_with(tail));
    // A date after the first call leaves it in the window all the same
    assert!(last(report(&lines, Some(at(120)))).ends_with(tail));
}

#[test]
fn a_minute_names_every_project_and_kind_in_it() {
    let home = tempfile::tempdir().unwrap();
    let kinds = [
        Kind::Issue,
        Kind::PullRequest,
        Kind::Reviewed,
        Kind::ReviewBot,
        Kind::MergeQueue,
        Kind::Viewer,
        Kind::CanPush,
    ];
    for (n, kind) in kinds.into_iter().enumerate() {
        let project = format!("p{n}");
        Ledger::in_home(home.path(), &project).record(Timestamp(START), kind, 5, Ended::Ok);
    }
    let out = report(&read(&home.path().join(FILE)).unwrap(), None);

    let minute = out
        .iter()
        .find(|l| l.starts_with("  2026-10-03T04:00:00Z:"))
        .unwrap();
    assert!(
        minute.starts_with(
            "  2026-10-03T04:00:00Z: 7 calls (p0 1, p1 1, p2 1, p3 1, p4 1, p5 1, p6 1)"
        )
    );
    for kind in kinds {
        assert!(minute.contains(&format!("{} 1", kind.name())), "{minute}");
    }
}

#[test]
fn a_line_written_before_the_identity_was_recorded_reads_as_the_logins() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(FILE);
    std::fs::write(
        &path,
        "{\"at\":1791000020,\"project\":\"shep\",\"kind\":\"pull_request\",\"api\":\"graphql\",\"millis\":80,\"ended\":\"ok\"}\n",
    )
    .unwrap();

    assert_eq!(read(&path).unwrap()[0].by, Identity::Login);
}

#[test]
fn the_report_splits_calls_between_the_login_and_the_app() {
    let home = tempfile::tempdir().unwrap();
    let koji = Ledger::in_home(home.path(), "koji");
    let at = |seconds: u64| Timestamp(START + seconds);
    koji.record(at(5), Kind::Reviewed, 5, Ended::Ok);
    for seconds in [10, 15, 20] {
        koji.record_by(
            Identity::App,
            at(seconds),
            Kind::OpenPullRequests,
            5,
            Ended::Ok,
        );
    }
    koji.record_by(Identity::App, at(70), Kind::Reviewed, 5, Ended::UsedUp);
    let lines = read(&home.path().join(FILE)).unwrap();
    assert_eq!(lines[1].by, Identity::App);

    let out = report(&lines, None);

    assert!(
        out.contains(
            &"by identity (calls, minutes with a call, a minute, busiest minute):".to_owned()
        ),
        "{out:#?}"
    );
    assert!(out.contains(&"  app: 4, 2, 2.0, 3".to_owned()), "{out:#?}");
    assert!(
        out.contains(&"  login: 1, 1, 1.0, 1".to_owned()),
        "{out:#?}"
    );
    // A used-up answer says whose budget it was
    assert!(
        out.last()
            .unwrap()
            .starts_with("  2026-10-03T04:01:10Z koji reviewed (graphql, app);"),
        "{out:#?}"
    );
}

#[test]
fn only_a_write_that_went_through_counts_as_one_made() {
    let home = tempfile::tempdir().unwrap();
    let ledger = Ledger::in_home(home.path(), "koji");
    let at = Timestamp(START);

    ledger.record(at, Kind::PullRequest, 80, Ended::Ok);
    ledger.record(at, Kind::Comment, 0, Ended::Held);
    ledger.record(at, Kind::Merge, 90, Ended::Failed);
    assert_eq!(ledger.writes(), 0);

    ledger.record(at, Kind::Comment, 80, Ended::Ok);
    ledger.record_by(Identity::App, at, Kind::AppSetIssueLabel, 80, Ended::Ok);
    assert_eq!(ledger.writes(), 2);
}

#[test]
fn a_call_that_came_back_304_says_so_and_a_line_without_one_reads_as_none() {
    let home = tempfile::tempdir().unwrap();
    let ledger = Ledger::in_home(home.path(), "koji");
    let at = Timestamp(START);
    ledger.record_with(Identity::Login, at, Kind::CommentsSince, 40, Ended::Ok, 2);
    ledger.record(at, Kind::Viewer, 40, Ended::Ok);

    let lines = read(&home.path().join(FILE)).unwrap();

    assert_eq!(lines[0].not_modified, 2);
    assert_eq!(lines[1].not_modified, 0);
    let text = std::fs::read_to_string(home.path().join(FILE)).unwrap();
    assert!(
        text.lines()
            .next()
            .unwrap()
            .ends_with(r#""not_modified":2}"#)
    );
    assert!(!text.lines().nth(1).unwrap().contains("not_modified"));
}

#[test]
fn the_report_counts_the_reads_that_came_back_304_by_kind() {
    let home = tempfile::tempdir().unwrap();
    let koji = Ledger::in_home(home.path(), "koji");
    let at = |seconds: u64| Timestamp(START + seconds);
    koji.record_with(Identity::Login, at(5), Kind::CommentsSince, 5, Ended::Ok, 1);
    koji.record_with(
        Identity::Login,
        at(10),
        Kind::CommentsSince,
        5,
        Ended::Ok,
        1,
    );
    koji.record_with(Identity::App, at(15), Kind::ReviewBot, 5, Ended::Ok, 3);
    koji.record(at(20), Kind::CommentsSince, 5, Ended::Ok);
    let lines = read(&home.path().join(FILE)).unwrap();

    let out = report(&lines, None);

    let at_line = out
        .iter()
        .position(|l| l.starts_with("answered 304"))
        .unwrap();
    assert_eq!(
        out[at_line],
        "answered 304 Not Modified (requests, by kind):"
    );
    assert_eq!(out[at_line + 1], "  review_bot: 3");
    assert_eq!(out[at_line + 2], "  comments_since: 2");
}

#[test]
fn a_report_with_no_304_has_no_section_for_them() {
    let home = tempfile::tempdir().unwrap();
    let lines = recorded(home.path());

    assert!(!report(&lines, None).iter().any(|l| l.contains("304")));
}
