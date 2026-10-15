// What goose reports of its context, the call that compacts its session, and
// the window a call is bounded to.

use std::num::NonZeroU32;

use super::hooks_tests::{env_vars, worker};
use super::output::Spent;
use super::tests::{FRESH, FRESH_ID, OTHER_ID, URL, World, goose_with, id, output, strings};
use super::*;
use crate::ports::Usage;
use crate::settings::ContextSize;

// Written by hand from the shapes in goose's source, because the builder had no
// model to record. #798 replaces them with recordings.
const COMPACTED: &str = include_str!("../../../fixtures/goose-compact.jsonl");
const COMPACT_FAILED: &str = include_str!("../../../fixtures/goose-compact-failed.jsonl");
// Three `systemNotification` messages, then the reply, and a `complete` total
// that holds the compaction's own request.
const AUTO_COMPACTED: &str = include_str!("../../../fixtures/goose-auto-compacted.jsonl");
const SESSION_LIST: &str = include_str!("../../../fixtures/goose-session-list.json");

const THRESHOLD: &str = "GOOSE_AUTO_COMPACT_THRESHOLD";

// What the session had spent before the call that printed the fixtures above.
const BEFORE: Spent = Spent {
    input_tokens: 9_000,
    output_tokens: 400,
    cache_read_input_tokens: 6_000,
    cache_write_input_tokens: 0,
    rebase: false,
};

// Every role, so a new one fails to compile here until it is placed.
fn every_role() -> [Role; 8] {
    const _: fn(Role) = |role| match role {
        Role::Worker
        | Role::Reviewer
        | Role::IssueWriter
        | Role::Pm
        | Role::Advisor
        | Role::Researcher
        | Role::Representative
        | Role::Triager => {}
    };
    [
        Role::Worker,
        Role::Reviewer,
        Role::IssueWriter,
        Role::Pm,
        Role::Advisor,
        Role::Researcher,
        Role::Representative,
        Role::Triager,
    ]
}

fn on_context(mut call: AgentCall, context: i64) -> AgentCall {
    let AgentHarness::Goose(server) = &mut call.harness else {
        unreachable!("the world's calls are goose's");
    };
    server.context = ContextSize::try_from(context).unwrap();
    call
}

fn windowed(call: AgentCall, context: i64, window: u32) -> AgentCall {
    let mut call = on_context(call, context);
    call.compact_window = NonZeroU32::new(window);
    call
}

fn threshold_of(call: &AgentCall) -> Option<String> {
    let prefix = format!("{THRESHOLD}=");
    let found: Vec<String> = env_vars(call)
        .into_iter()
        .filter_map(|v| v.strip_prefix(&prefix).map(str::to_owned))
        .collect();
    assert!(found.len() <= 1, "{found:?}");
    found.into_iter().next()
}

// goose as a script that prints `listed` for `session list`, exiting with
// `list_code`, and for anything else keeps its arguments and prints `stdout`.
// The arguments of a list go to `list-argv`, which exists only if goose listed.
fn goose_listing(world: &World, stdout: &str, listed: &str, list_code: u8) -> GooseCli {
    let (out, list) = (world.path("out"), world.path("list"));
    std::fs::write(&out, stdout).unwrap();
    std::fs::write(&list, listed).unwrap();
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = session ]; then\n  printf '%s\\n' \"$@\" > {}\n  cat {}\n  \
         exit {list_code}\nfi\nprintf '%s\\n' \"$@\" > {}\ncat {}\n",
        world.path("list-argv").display(),
        list.display(),
        world.path("argv").display(),
        out.display(),
    );
    let program = world.path("goose");
    crate::test::write_script(&program, &script);
    goose_with(world, program)
}

fn worker_call(world: &World) -> AgentCall {
    worker(world, Session::New(id(FRESH_ID)))
}

fn context_after(world: &World, call: &AgentCall, listed: &str, list_code: u8) -> Option<u64> {
    let goose = goose_listing(world, FRESH, listed, list_code);
    let reply = goose.run(call, &Ending::default()).unwrap();
    reply.context
}

#[test]
fn a_worker_turn_s_context_is_the_session_s_last_request_input_from_goose_session_list() {
    let w = World::new();
    let call = worker_call(&w);
    assert_eq!(context_after(&w, &call, SESSION_LIST, 0), Some(48_213));
    let listed = std::fs::read_to_string(w.path("list-argv")).unwrap();
    assert_eq!(listed, "session\nlist\n--format\njson\n");
}

#[test]
fn a_failed_list_or_a_list_without_the_session_or_its_tokens_reports_no_context() {
    let named =
        |usage: &str| format!(r#"[{{"id":"20261013_2","name":"{FRESH_ID}","usage":{usage}}}]"#);
    let cases = [
        ("a list that exits 1", SESSION_LIST.to_owned(), 1),
        ("a list that is not JSON", "no sessions".to_owned(), 0),
        ("an empty list", "[]".to_owned(), 0),
        (
            "other sessions only",
            SESSION_LIST.replace(FRESH_ID, "other"),
            0,
        ),
        (
            "a session with no usage",
            format!(r#"[{{"id":"{FRESH_ID}"}}]"#),
            0,
        ),
        ("a usage with no tokens", named("{}"), 0),
        (
            "a usage with null tokens",
            named(r#"{"input_tokens":null}"#),
            0,
        ),
    ];
    for (name, listed, code) in cases {
        let w = World::new();
        let call = worker_call(&w);
        assert_eq!(context_after(&w, &call, &listed, code), None, "{name}");
    }
}

#[test]
fn only_a_worker_call_reads_its_context() {
    for role in every_role() {
        let w = World::new();
        let call = AgentCall {
            role,
            ..worker_call(&w)
        };
        let context = context_after(&w, &call, SESSION_LIST, 0);
        let listed = w.path("list-argv").exists();
        assert_eq!(
            (context, listed),
            match role {
                Role::Worker => (Some(48_213), true),
                _ => (None, false),
            },
            "{role:?}"
        );
    }
}

#[test]
fn a_compact_call_resumes_the_session_sends_the_compact_command_and_skips_the_skill_mapping() {
    let w = World::new();
    std::fs::write(w.path("instructions.md"), "You build.").unwrap();
    let mut call = worker(&w, Session::Resume(id(FRESH_ID)));
    call.prompt = COMPACT.to_owned();
    call.instructions = Some(w.path("instructions.md"));
    let expected = [
        "run",
        "--no-profile",
        "--output-format",
        "stream-json",
        "--text=/compact",
        "--resume",
        "--name",
        FRESH_ID,
    ];
    assert_eq!(strings(&call), expected);
}

#[test]
fn a_compact_call_that_compacted_replies_with_its_usage_and_no_text() {
    let w = World::new();
    let call = AgentCall {
        prompt: COMPACT.to_owned(),
        ..worker(&w, Session::Resume(id(FRESH_ID)))
    };
    let out = output(0, COMPACTED, "");
    let reply = compact::read(true, &out, &call.session, BEFORE)
        .reply
        .unwrap();
    assert_eq!(reply.text, "");
    assert_eq!(reply.context, None);
    assert_eq!(reply.session_id, id(FRESH_ID));
    // The compaction's own request, which no message carries.
    let spent = Usage {
        input: 9_200,
        output: 310,
        ..Usage::default()
    };
    assert_eq!(reply.usage, spent);
}

#[test]
fn a_compact_call_that_printed_an_error_is_a_failed_call_that_says_why() {
    let session = Session::Resume(id(FRESH_ID));
    let out = output(0, COMPACT_FAILED, "");
    let err = compact::read(true, &out, &session, BEFORE)
        .reply
        .unwrap_err();
    assert!(
        matches!(&err, AgentError::Failed(_, why) if why.contains("context length error")),
        "{err:?}"
    );
    // Any other turn's reply is the model's own words.
    let turn = compact::read(false, &out, &session, BEFORE).reply.unwrap();
    assert!(turn.text.contains("Error compacting"), "{turn:?}");
}

#[test]
fn a_compact_call_with_no_assistant_message_is_unreadable() {
    let session = Session::Resume(id(FRESH_ID));
    let silent: String = COMPACTED
        .lines()
        .filter(|l| !l.contains(r#""type":"message""#))
        .collect();
    let out = output(0, &silent, "");
    let err = compact::read(true, &out, &session, BEFORE)
        .reply
        .unwrap_err();
    assert!(matches!(err, AgentError::Unreadable(..)), "{err:?}");

    // An assistant message with no text is no answer either.
    let notice = AUTO_COMPACTED
        .lines()
        .filter(|l| l.contains("inlineMessage"))
        .take(1);
    let out = output(0, &notice.collect::<String>(), "");
    let err = compact::read(true, &out, &session, BEFORE)
        .reply
        .unwrap_err();
    assert!(matches!(err, AgentError::Unreadable(..)), "{err:?}");
}

#[test]
fn a_turn_goose_compacted_by_itself_counts_the_compaction_in_its_usage() {
    let session = Session::Resume(id(FRESH_ID));
    let out = output(0, AUTO_COMPACTED, "");
    let read = compact::read(false, &out, &session, BEFORE);
    let reply = read.reply.unwrap();
    assert_eq!(reply.text, "The tests pass and the change is pushed.");
    // The totals after the turn less those before it: the compaction's request
    // and the turn's own.
    let spent = Usage {
        input: 36_200,
        output: 1_930,
        ..Usage::default()
    };
    assert_eq!(reply.usage, spent);
    assert_eq!(read.spent.map(|s| s.input_tokens), Some(45_200));
}

#[test]
fn a_window_below_eight_tenths_of_the_context_sets_the_threshold_to_the_fraction_rounded_up() {
    let w = World::new();
    let table = [(262_144, 200_000, "0.7630"), (262_144, 100_000, "0.3815")];
    for (context, window, fraction) in table {
        let call = windowed(worker_call(&w), context, window);
        assert_eq!(
            threshold_of(&call).as_deref(),
            Some(fraction),
            "{context}/{window}"
        );
    }
    // Whatever is set is under 1, which goose reads as off, and never under the window.
    for context in [4_096_u32, 65_536, 131_072, 262_144, 1_000_000] {
        for window in (1..context).step_by(997) {
            let call = windowed(worker_call(&w), i64::from(context), window);
            let Some(value) = threshold_of(&call) else {
                continue;
            };
            let value: f64 = value.parse().unwrap();
            assert!(value > 0.0 && value <= 0.8, "{context}/{window}: {value}");
            assert!(
                value >= f64::from(window) / f64::from(context),
                "{context}/{window}"
            );
        }
    }
}

#[test]
fn a_window_at_or_past_eight_tenths_of_the_context_leaves_the_threshold_unset() {
    let w = World::new();
    let table = [(64_000, 200_000), (200_000, 190_000), (200_000, 160_000)];
    for (context, window) in table {
        let call = windowed(worker_call(&w), context, window);
        assert_eq!(threshold_of(&call), None, "{context}/{window}");
    }
}

#[test]
fn a_call_with_no_window_and_a_compact_call_set_no_threshold() {
    let w = World::new();
    let call = on_context(worker_call(&w), 262_144);
    assert_eq!(threshold_of(&call), None);
    let compacting = AgentCall {
        prompt: COMPACT.to_owned(),
        ..windowed(worker(&w, Session::Resume(id(FRESH_ID))), 262_144, 100_000)
    };
    assert_eq!(threshold_of(&compacting), None);
}

#[test]
fn every_goose_call_carries_the_models_context_as_goose_context_limit() {
    let w = World::new();
    for role in every_role() {
        let sessions = [Session::New(id(OTHER_ID)), Session::Resume(id(OTHER_ID))];
        for session in sessions {
            let base = AgentCall {
                role,
                ..w.call(URL, role, session.clone())
            };
            let compacting = AgentCall {
                prompt: COMPACT.to_owned(),
                ..base.clone()
            };
            for call in [base.clone(), windowed(base, 262_144, 100_000), compacting] {
                let set: Vec<String> = (env_vars(&call).into_iter())
                    .filter(|v| v.starts_with("GOOSE_CONTEXT_LIMIT="))
                    .collect();
                let AgentHarness::Goose(server) = &call.harness else {
                    unreachable!("the world's calls are goose's");
                };
                let want = format!("GOOSE_CONTEXT_LIMIT={}", server.context.get());
                assert_eq!(set, [want], "{role:?}");
            }
        }
    }
}
