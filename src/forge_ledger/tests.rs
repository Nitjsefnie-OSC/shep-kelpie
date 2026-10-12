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
            millis: 80,
            ended: Ended::Ok,
        }
    );
    let text = std::fs::read_to_string(home.path().join(FILE)).unwrap();
    assert_eq!(
        text.lines().nth(3).unwrap(),
        r#"{"at":1791000020,"project":"shep","kind":"pull_request","api":"graphql","millis":80,"ended":"ok"}"#
    );
    assert!(text.contains(r#""kind":"viewer","api":"rest","millis":0,"ended":"held""#));
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
