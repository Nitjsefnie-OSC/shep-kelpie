use std::path::PathBuf;

use super::*;
use crate::ports::{CLEAN, Reviewer, Severity};
use crate::settings::{ContextSize, EndpointUrl, LocalRound, ModelHost, NonBlank};
use crate::test::{
    Answer, Elsewhere, StandInEndpoint, cut_from_main, git, linked_worktree, main_commits,
    unreachable_url,
};

const DIFF: &str = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -3,4 +3,5 @@ fn a() {
 keep
--- not a header
+added
+++ not a header either
 tail
\\ No newline at end of file
diff --git a/gone.rs b/gone.rs
deleted file mode 100644
--- a/gone.rs
+++ /dev/null
@@ -1,1 +0,0 @@
-old
diff --git a/logo.png b/logo.png
Binary files a/logo.png and b/logo.png differ
";

#[test]
fn a_diff_reads_as_numbered_hunks_per_file() {
    assert_eq!(
        parse(DIFF),
        vec![
            FileDiff {
                path: "src/lib.rs".into(),
                hunks: vec![
                    "@@ -3,4 +3,5 @@ fn a() {\n\
                     \x20    3  keep\n\
                     \x20      --- not a header\n\
                     \x20    4 +added\n\
                     \x20    5 +++ not a header either\n\
                     \x20    6  tail\n"
                        .into()
                ],
            },
            FileDiff {
                path: "gone.rs".into(),
                hunks: vec!["@@ -1,1 +0,0 @@\n       -old\n".into()],
            },
        ]
    );
}

#[test]
fn hunks_pack_into_chunks_that_name_their_file() {
    let file = |path: &str, hunks: &[&str]| FileDiff {
        path: path.into(),
        hunks: hunks.iter().map(|h| (*h).to_owned()).collect(),
    };
    let files = [file("a", &["1111\n", "2222\n"]), file("b", &["3333\n"])];
    assert_eq!(
        chunks(&files, 100),
        ["### a\n1111\n2222\n### b\n3333\n"],
        "everything fits in one"
    );
    assert_eq!(
        chunks(&files, 16),
        ["### a\n1111\n2222\n", "### b\n3333\n"],
        "a file starts a new chunk once the old one is full"
    );
    assert_eq!(
        chunks(&files, 11),
        ["### a\n1111\n", "### a\n2222\n", "### b\n3333\n"],
        "a file split across chunks is named in each"
    );
}

#[test]
fn a_hunk_too_big_for_a_chunk_is_cut_between_lines() {
    let big = FileDiff {
        path: "a".into(),
        hunks: vec!["one\ntwo\nthree\n".into()],
    };
    assert_eq!(chunks(&[big], 14), ["### a\none\ntwo\n", "### a\nthree\n"]);
    let line = FileDiff {
        path: "a".into(),
        hunks: vec!["a line longer than the budget\n".into()],
    };
    assert_eq!(chunks(&[line], 10).len(), 1, "a line too long still goes");
}

#[test]
fn a_new_start_line_is_read_from_the_hunk_header() {
    assert_eq!(new_start("@@ -12,7 +14,9 @@ fn name"), 14);
    assert_eq!(new_start("@@ -1 +1 @@"), 1);
    assert_eq!(new_start("@@ -1,1 +0,0 @@"), 0);
}

// A repo with `origin/main` at its first commit and one commit on top.
// A repo and a worktree of it whose commit changes `src/lib.rs` to `change`
fn repo(dir: &Path, change: &str) -> (PathBuf, PathBuf) {
    let (repo, worktree) = linked_worktree(dir);
    std::fs::create_dir_all(worktree.join("src")).unwrap();
    std::fs::write(worktree.join("src/lib.rs"), "fn a() {}\n").unwrap();
    git(&worktree, &["add", "."]);
    git(&worktree, &["commit", "--quiet", "-m", "init"]);
    let base = git(&worktree, &["rev-parse", "HEAD"]);
    git(
        &worktree,
        &["update-ref", "refs/remotes/origin/main", &base],
    );
    std::fs::write(worktree.join("src/lib.rs"), change).unwrap();
    git(&worktree, &["commit", "--quiet", "-am", "change"]);
    (repo, worktree)
}

fn linked((repo, worktree): &(PathBuf, PathBuf)) -> Linked<'_> {
    Linked { repo, worktree }
}

fn local(url: &str, context: u32) -> LocalRound {
    LocalRound::Endpoint(Endpoint {
        host: ModelHost::Url(EndpointUrl::try_from(url.to_owned()).unwrap()),
        model: NonBlank::try_from("coder".to_owned()).unwrap(),
        context: ContextSize::try_from(i64::from(context)).unwrap(),
        prompt: None,
    })
}

#[test]
fn a_round_sends_the_diff_with_the_prompt_and_reads_the_findings() {
    let dir = tempfile::tempdir().unwrap();
    let worktree = repo(dir.path(), "fn a() {}\nfn b() { panic!() }\n");
    let server = StandInEndpoint::start([Answer::Says(
        "<think>HIGH|src/lib.rs:1|a thought|not a finding</think>\n\
         MEDIUM|src/lib.rs:2|b panics|callers crash",
    )]);
    let out = dir.path().join("out");
    let findings = LocalReviewer::default()
        .round(
            &local(server.url(), 8192),
            linked(&worktree),
            "origin/main",
            &out,
            3,
            "",
        )
        .unwrap();
    assert_eq!(
        findings,
        vec![Finding {
            severity: Severity::Medium,
            file: "src/lib.rs".into(),
            line: 2,
            what: "b panics".into(),
            why: "callers crash".into(),
            test: None,
        }]
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request["model"], "coder");
    assert_eq!(request["temperature"], 0);
    assert_eq!(request["max_tokens"], 2048);
    assert_eq!(request["messages"][0]["content"], RECORDED);
    let diff = request["messages"][1]["content"].as_str().unwrap();
    assert!(diff.starts_with("### src/lib.rs\n@@ "), "{diff}");
    assert!(diff.contains("     2 +fn b() { panic!() }\n"), "{diff}");
    assert!(out.join("round-3.txt.done").is_file());
    assert!(out.join("round-3/reply-0.json").is_file());
}

// The system prompt every endpoint reviewer was sent before a body could
// replace part of it, kept as it was recorded.
const RECORDED: &str = include_str!("recorded-prompt.md");

const CRITERIA_LEAD: &str = "\nThe issue this pull request resolves asks for the following. \
                             Use it to judge what the change is meant to do.\n\n";

// The recorded prompt from the paragraph starting `from`, to the one starting `to`.
fn recorded_between(from: &str, to: Option<&str>) -> &'static str {
    let start = RECORDED.find(from).unwrap();
    let end = to.map_or(RECORDED.len(), |to| RECORDED.find(to).unwrap());
    &RECORDED[start..end]
}

const DIFF_FORMAT_STARTS: &str = "The diff is below, one section per file.";
const WHAT_TO_REPORT_STARTS: &str = "Report what would break";
const ANSWER_FORMAT_STARTS: &str = "Write one line per finding";

// What the single chunk of a one-file change is sent as the system prompt,
// when the reviewer has `body` and the issue asks `criteria`.
fn system_sent(body: Option<&str>, criteria: &str, replies: &[&'static str]) -> String {
    let dir = tempfile::tempdir().unwrap();
    let worktree = repo(dir.path(), "fn a() {}\nfn b() {}\n");
    let server = StandInEndpoint::start(replies.iter().map(|r| Answer::Says(r)));
    let LocalRound::Endpoint(mut endpoint) = local(server.url(), 8192) else {
        unreachable!()
    };
    endpoint.prompt = body.map(str::to_owned);
    LocalReviewer::default()
        .round(
            &LocalRound::Endpoint(endpoint),
            linked(&worktree),
            "origin/main",
            &dir.path().join("out"),
            1,
            criteria,
        )
        .unwrap();
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    requests[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn an_endpoint_reviewer_with_no_body_sends_the_text_it_sent_before() {
    assert_eq!(system_sent(None, "", &[CLEAN]), RECORDED);
    assert_eq!(
        system_sent(None, "  Add b.\n", &[CLEAN]),
        format!("{RECORDED}{CRITERIA_LEAD}Add b.\n")
    );
}

#[test]
fn an_endpoint_chunk_s_request_carries_the_body_then_the_diff_and_answer_format_then_the_criteria()
{
    let body = "Report only a panic a caller can reach.";
    let sent = system_sent(Some(body), "Add b.", &[CLEAN]);
    let diff_format = recorded_between(DIFF_FORMAT_STARTS, Some(WHAT_TO_REPORT_STARTS));
    let answer_format = recorded_between(ANSWER_FORMAT_STARTS, None);
    assert_eq!(
        sent,
        format!("{body}\n\n{diff_format}{answer_format}{CRITERIA_LEAD}Add b.\n")
    );
    assert!(!sent.contains("You review one part"), "{sent}");
    assert!(!sent.contains(WHAT_TO_REPORT_STARTS), "{sent}");
}

#[test]
fn a_body_that_asks_for_another_answer_format_is_still_read_in_kelpie_s_format() {
    let dir = tempfile::tempdir().unwrap();
    let worktree = repo(dir.path(), "fn a() {}\nfn b() {}\n");
    let server = StandInEndpoint::start([
        Answer::Says("MEDIUM|src/lib.rs:2|b is new|it matters"),
        Answer::Says(r#"{"findings":[{"line":2,"what":"b is new"}]}"#),
    ]);
    let LocalRound::Endpoint(mut endpoint) = local(server.url(), 8192) else {
        unreachable!()
    };
    endpoint.prompt = Some("Answer in JSON, as {\"findings\": []}.".into());
    let round = LocalRound::Endpoint(endpoint);
    let reviewer = LocalReviewer::default();
    let out = dir.path().join("out");

    let findings = reviewer
        .round(&round, linked(&worktree), "origin/main", &out, 1, "")
        .unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, 2);

    let url = server.url();
    assert_eq!(
        reviewer.round(&round, linked(&worktree), "origin/main", &out, 2, ""),
        Err(ReviewerError::Failed(format!(
            "{url}'s reply is neither findings nor {CLEAN}: \
             {{\"findings\":[{{\"line\":2,\"what\":\"b is new\"}}]}}"
        )))
    );
}

#[test]
fn main_gaining_a_commit_after_the_branch_is_cut_leaves_the_diff_sent_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (checkout, worktree) = linked_worktree(dir.path());
    main_commits(&checkout, "src/lib.rs", "fn a() {}\n");
    cut_from_main(&worktree);
    std::fs::write(worktree.join("src/lib.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    git(&worktree, &["commit", "--quiet", "-am", "change"]);
    main_commits(&checkout, "src/lib.rs", "fn only_main() {}\nfn a() {}\n");
    let server = StandInEndpoint::start([Answer::Says(CLEAN)]);

    LocalReviewer::default()
        .round(
            &local(server.url(), 8192),
            linked(&(checkout, worktree)),
            "origin/main",
            &dir.path().join("out"),
            1,
            "",
        )
        .unwrap();

    let requests = server.requests();
    let diff = requests[0]["messages"][1]["content"].as_str().unwrap();
    assert!(diff.contains("+fn b() {}"), "{diff}");
    assert!(!diff.contains("only_main"), "{diff}");
}

#[test]
fn a_diff_bigger_than_the_context_goes_in_several_requests() {
    let dir = tempfile::tempdir().unwrap();
    let big: String = (0..2000).map(|i| format!("fn f{i}() {{}}\n")).collect();
    let worktree = repo(dir.path(), &big);
    let server = StandInEndpoint::start([]);
    let out = dir.path().join("out");
    let findings = LocalReviewer::default()
        .round(
            &local(server.url(), 4096),
            linked(&worktree),
            "origin/main",
            &out,
            1,
            "",
        )
        .unwrap();
    assert!(findings.is_empty(), "CLEAN from every chunk");
    let requests = server.requests();
    assert!(requests.len() > 1, "{} requests", requests.len());
    let reply = 4096 / REPLY_SHARE;
    let prompt = RECORDED.len().div_ceil(BYTES_PER_TOKEN) + FRAMING_TOKENS;
    let budget = (4096 - reply - prompt) * BYTES_PER_TOKEN;
    for request in &requests {
        let diff = request["messages"][1]["content"].as_str().unwrap();
        assert!(diff.len() <= budget, "{} > {budget}", diff.len());
        assert!(diff.starts_with("### src/lib.rs\n"), "{diff}");
    }
}

#[test]
fn an_empty_diff_asks_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let worktree = repo(dir.path(), "fn a() {}\nfn b() {}\n");
    let server = StandInEndpoint::start([]);
    let out = dir.path().join("out");
    let findings = LocalReviewer::default()
        .round(
            &local(server.url(), 8192),
            linked(&worktree),
            "HEAD",
            &out,
            1,
            "",
        )
        .unwrap();
    assert!(findings.is_empty());
    assert!(server.requests().is_empty());
    assert!(out.join("round-1.txt.done").is_file());
}

#[test]
fn a_refused_or_unreadable_reply_fails_the_round() {
    let dir = tempfile::tempdir().unwrap();
    let worktree = repo(dir.path(), "fn a() {}\nfn b() {}\n");
    let server = StandInEndpoint::start([
        Answer::Status(500, r#"{"error":"model not loaded"}"#),
        Answer::Status(200, "not json"),
    ]);
    let reviewer = LocalReviewer::default();
    let local = local(server.url(), 8192);
    let out = dir.path().join("out");
    let err = reviewer
        .round(&local, linked(&worktree), "origin/main", &out, 1, "")
        .unwrap_err();
    let url = format!("{}/chat/completions", server.url());
    assert_eq!(
        err,
        ReviewerError::Failed(format!(
            "{url} answered HTTP 500: {{\"error\":\"model not loaded\"}}"
        ))
    );
    let err = reviewer
        .round(&local, linked(&worktree), "origin/main", &out, 2, "")
        .unwrap_err();
    assert_eq!(err, ReviewerError::Unreadable("not json".into()));
}

#[test]
fn a_reply_cut_off_while_thinking_fails_the_round() {
    let dir = tempfile::tempdir().unwrap();
    let worktree = repo(dir.path(), "fn a() {}\nfn b() {}\n");
    let server = StandInEndpoint::start([Answer::Says(
        "<think>HIGH|src/lib.rs:2|maybe this|or not, let me check the",
    )]);
    let out = dir.path().join("out");
    let err = LocalReviewer::default()
        .round(
            &local(server.url(), 8192),
            linked(&worktree),
            "origin/main",
            &out,
            1,
            "",
        )
        .unwrap_err();
    let url = format!("{}/chat/completions", server.url());
    assert_eq!(
        err,
        ReviewerError::Failed(format!(
            "{url}'s reply ended inside its thinking, so nothing was reviewed"
        ))
    );
}

#[test]
fn a_reply_with_no_findings_that_is_not_clean_fails_the_round() {
    let dir = tempfile::tempdir().unwrap();
    let worktree = repo(dir.path(), "fn a() {}\nfn b() {}\n");
    let server = StandInEndpoint::start([
        Answer::Says(""),
        Answer::Says("<think>long thoughts</think>"),
        Answer::Says("The code looks fine."),
        Answer::Says("  CLEAN\n"),
    ]);
    let reviewer = LocalReviewer::default();
    let local = local(server.url(), 8192);
    let out = dir.path().join("out");
    let url = server.url();
    for said in ["", "", "The code looks fine."] {
        assert_eq!(
            reviewer.round(&local, linked(&worktree), "origin/main", &out, 1, ""),
            Err(ReviewerError::Failed(format!(
                "{url}'s reply is neither findings nor {CLEAN}: {said}"
            )))
        );
        assert!(!out.join("round-1.txt.done").exists());
    }
    assert_eq!(
        reviewer.round(&local, linked(&worktree), "origin/main", &out, 1, ""),
        Ok(vec![])
    );
}

#[test]
fn a_retried_round_never_reads_the_last_tries_reply() {
    let dir = tempfile::tempdir().unwrap();
    let worktree = repo(dir.path(), "fn a() {}\nfn b() {}\n");
    let server = StandInEndpoint::start([
        Answer::Says("MEDIUM|src/lib.rs:2|b is new|it matters"),
        Answer::Status(200, ""),
    ]);
    let reviewer = LocalReviewer::default();
    let local = local(server.url(), 8192);
    let out = dir.path().join("out");
    let first = reviewer.round(&local, linked(&worktree), "origin/main", &out, 1, "");
    assert_eq!(first.unwrap().len(), 1);
    let again = reviewer.round(&local, linked(&worktree), "origin/main", &out, 1, "");
    assert_eq!(again, Err(ReviewerError::Unreadable(String::new())));
}

#[test]
fn the_start_check_asks_the_server_for_its_models() {
    let server = StandInEndpoint::start([]);
    let reviewer = LocalReviewer::default();
    assert_eq!(reviewer.check(&local(server.url(), 8192)), Ok(()));
    assert!(server.requests().is_empty(), "no chat request was sent");
    let url = unreachable_url();
    let err = reviewer.check(&local(&url, 8192)).unwrap_err();
    assert!(
        err.starts_with(&format!("cannot reach {url}/models: ")),
        "{err}"
    );
}

// The round runs outside the worker's sandbox, and its worker can write
// the `commondir` git follows to the repo whose config it reads.
#[test]
fn a_worktree_whose_commondir_points_elsewhere_is_refused_and_runs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let checkout = repo(dir.path(), "fn a() {}\nfn b() {}\n");
    let elsewhere = Elsewhere::copy_of(&checkout.0, dir.path());
    elsewhere.as_common_dir_of(&checkout.1);
    let server = StandInEndpoint::start([]);
    let out = dir.path().join("out");

    let err = LocalReviewer::default()
        .round(
            &local(server.url(), 8192),
            linked(&checkout),
            "origin/main",
            &out,
            1,
            "",
        )
        .unwrap_err();
    let ReviewerError::Failed(why) = err else {
        panic!("{err:?}")
    };
    assert!(why.contains("is not this work item's worktree"), "{why}");
    assert!(!elsewhere.ran.exists(), "the round started the program");
    assert!(server.requests().is_empty(), "nothing was reviewed");
    elsewhere.assert_plain_git_starts_it(&checkout.1);
}

#[test]
fn a_round_behind_a_gateway_sends_its_key_from_curls_stdin_and_holds_a_lease() {
    use crate::settings::{Gateway, GatewayKind, GatewayName, Gateways, KeyVar};
    let dir = tempfile::tempdir().unwrap();
    let checkout = repo(dir.path(), "fn a() {}\nfn b() {}\n");
    let server = StandInEndpoint::start([Answer::Says(CLEAN)]).like_paddock("pk-r", &[]);
    let name = GatewayName::try_from("paddock".to_owned()).unwrap();
    let gateway = Gateway {
        kind: GatewayKind::Paddock,
        url: EndpointUrl::try_from(server.host().to_owned()).unwrap(),
        key_env: KeyVar::try_from("PADDOCK_KEY".to_owned()).unwrap(),
    };
    let table = std::collections::BTreeMap::from([(name.clone(), gateway)]);
    let reviewer =
        LocalReviewer::default().with_gateways(Gateways::reading(table, |_| Some("pk-r".into())));
    let LocalRound::Endpoint(mut endpoint) = local("http://unused/v1", 8192) else {
        unreachable!()
    };
    endpoint.host = ModelHost::Gateway(name);
    let round = LocalRound::Endpoint(endpoint);
    reviewer.check(&round).unwrap();
    let out = dir.path().join("out");
    let findings = reviewer.round(&round, linked(&checkout), "origin/main", &out, 1, "");
    assert_eq!(findings, Ok(Vec::new()));
    assert_eq!(
        server.seen(),
        [
            "GET /v1/models HTTP/1.1",
            "POST /paddock/leases HTTP/1.0",
            "POST /v1/chat/completions HTTP/1.1",
            "DELETE /paddock/leases/L1 HTTP/1.0",
        ],
        "the lease is taken before the chunk and released after it"
    );
    assert_eq!(server.authorizations()[2].as_deref(), Some("Bearer pk-r"));
}

#[test]
fn a_round_behind_a_gateway_asks_for_batch_on_its_chat_call_alone_and_a_plain_url_sends_none() {
    use crate::settings::{Gateway, GatewayKind, GatewayName, Gateways, KeyVar};
    let dir = tempfile::tempdir().unwrap();
    let checkout = repo(dir.path(), "fn a() {}\nfn b() {}\n");
    let server = StandInEndpoint::start([Answer::Says(CLEAN)]).like_paddock("pk-r", &[]);
    let name = GatewayName::try_from("paddock".to_owned()).unwrap();
    let gateway = Gateway {
        kind: GatewayKind::Paddock,
        url: EndpointUrl::try_from(server.host().to_owned()).unwrap(),
        key_env: KeyVar::try_from("PADDOCK_KEY".to_owned()).unwrap(),
    };
    let table = std::collections::BTreeMap::from([(name.clone(), gateway)]);
    let reviewer =
        LocalReviewer::default().with_gateways(Gateways::reading(table, |_| Some("pk-r".into())));
    let LocalRound::Endpoint(mut endpoint) = local("http://unused/v1", 8192) else {
        unreachable!()
    };
    endpoint.host = ModelHost::Gateway(name);
    let round = LocalRound::Endpoint(endpoint);
    reviewer.check(&round).unwrap();
    let out = dir.path().join("out");
    let findings = reviewer.round(&round, linked(&checkout), "origin/main", &out, 1, "");
    assert_eq!(findings, Ok(Vec::new()));
    let seen = server.seen();
    let chat = seen
        .iter()
        .position(|l| l.contains("/chat/completions"))
        .unwrap();
    let batch = vec![("x-paddock-priority".to_owned(), "batch".to_owned())];
    for (at, headers) in server.paddock_headers().into_iter().enumerate() {
        let expected = if at == chat {
            batch.clone()
        } else {
            Vec::new()
        };
        assert_eq!(headers, expected, "{}", seen[at]);
    }

    let plain = StandInEndpoint::start([Answer::Says(CLEAN)]);
    let reviewer = LocalReviewer::default();
    let findings = reviewer.round(
        &local(plain.url(), 8192),
        linked(&checkout),
        "origin/main",
        &out,
        1,
        "",
    );
    assert_eq!(findings, Ok(Vec::new()));
    assert_eq!(plain.paddock_headers(), [Vec::<(String, String)>::new()]);
}

#[test]
fn a_round_waiting_for_its_turn_behind_a_gateway_queues_until_it_is_granted() {
    use crate::adapters::paddock::Timing;
    use crate::ports::RoundStage;
    use crate::settings::{Gateway, GatewayKind, GatewayName, Gateways, KeyVar};
    use crate::test::Take;
    const TURN: &str =
        r#"{"error":"busy","reason":"coder is serving kelpie #9","reason_kind":"turn"}"#;
    const LOADING: &str = r#"{"error":"busy","reason":"coder is loading","reason_kind":"loading"}"#;
    let dir = tempfile::tempdir().unwrap();
    let checkout = repo(dir.path(), "fn a() {}\nfn b() {}\n");
    let server = StandInEndpoint::start([Answer::Says(CLEAN)])
        .like_paddock("pk-r", &[])
        .with_takes([
            Take::Status(503, LOADING),
            Take::Status(503, TURN),
            Take::Status(503, TURN),
        ]);
    let name = GatewayName::try_from("paddock".to_owned()).unwrap();
    let gateway = Gateway {
        kind: GatewayKind::Paddock,
        url: EndpointUrl::try_from(server.host().to_owned()).unwrap(),
        key_env: KeyVar::try_from("PADDOCK_KEY".to_owned()).unwrap(),
    };
    let table = std::collections::BTreeMap::from([(name.clone(), gateway)]);
    let quick = Timing {
        retry: std::time::Duration::from_millis(10),
        ..Timing::RUNNER
    };
    let reviewer = LocalReviewer::default()
        .with_gateways(Gateways::reading(table, |_| Some("pk-r".into())))
        .with_paddock_timing(quick);
    let LocalRound::Endpoint(mut endpoint) = local("http://unused/v1", 8192) else {
        unreachable!()
    };
    endpoint.host = ModelHost::Gateway(name);
    let round = LocalRound::Endpoint(endpoint);
    let stages = std::sync::Mutex::new(Vec::new());
    let watch = |stage| stages.lock().unwrap().push(stage);
    let out = dir.path().join("out");
    let findings = reviewer.round_watched(
        &round,
        linked(&checkout),
        "origin/main",
        &out,
        1,
        "",
        &watch,
    );
    assert_eq!(findings, Ok(Vec::new()));
    assert_eq!(
        *stages.lock().unwrap(),
        [RoundStage::Queued, RoundStage::Running],
        "a load is no queue, a turn is, and the grant ends it once"
    );
}
