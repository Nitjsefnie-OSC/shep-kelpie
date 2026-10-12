//! The stand-in Claude: what it answers, and the calls it saw

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::{FakeMeter, git, write_in};
use crate::ports::{
    AgentCall, AgentError, AgentReply, Agents, CallActivity, Cost, Ending, Role, Usage, Utilization,
};

/// How often a held call looks whether it was ended, as the real adapters
/// look at their child
const LOOK_EVERY: Duration = Duration::from_millis(5);

/// The file a killed worker leaves in its worktree, to find after a restart
pub(crate) const LEFT_BEHIND: &str = "left-behind.txt";

/// What the stand-in Claude does with its next call
#[derive(Debug, Clone)]
pub(crate) enum Scripted {
    /// Answers with this usage, and this cost for the session so far
    Reply(Usage, Cost),
    /// Answers with this usage and no cost, as a harness that reports
    /// none does
    Tokens(Usage),
    /// Answers like [`Self::Reply`], with the account's usage as given by
    /// the time it does: the call itself spent it
    Spend(Utilization, Usage, Cost),
    /// Fails with this error
    Fail(AgentError),
    /// Leaves [`LEFT_BEHIND`] in the worktree, then dies with the runner
    Kill,
    /// Commits this file with this text on the worktree's branch, pushes
    /// it the way a worker does, and answers
    Push(&'static str, &'static str),
    /// Pushes like [`Self::Push`], and answers with its session's context
    /// this many tokens
    PushAt(&'static str, &'static str, u64),
    /// Commits this file with this text on the worktree's branch, pushes
    /// nothing, and answers
    Commit(&'static str, &'static str),
    /// Commits these files with these texts in one commit on the worktree's
    /// branch, pushes nothing, and answers
    CommitFiles(&'static [(&'static str, &'static str)]),
    /// Commits this file with this text on the worktree's branch, pushes
    /// it the way a worker does, and answers with this final message
    PushSay(&'static str, &'static str, &'static str),
    /// Pushes like [`Self::PushSay`] from a session Claude Code compacted
    /// partway, which answers with the final message only if the settings
    /// file's `SessionStart` hook for a compaction handed it the reply's
    /// `thread <n>: fixed` line format back, and with "done" otherwise
    PushSayCompacted(&'static str, &'static str, &'static str),
    /// Writes this file with this text in the worktree, commits nothing,
    /// and answers: a write that got past the fence
    Plant(&'static str, &'static str),
    /// Stages this file with this text, commits nothing, and deletes the
    /// working copy, so only the index holds it, and answers
    Stage(&'static str, &'static str),
    /// Merges `origin/main` into the worktree's branch, keeping main's
    /// side of any conflict, and pushes it without force, as a worker
    /// resolving a conflict does
    MergeMain,
    /// Answers with this exact text and no cost: a review round, whose
    /// reply is read rather than acted on
    Text(&'static str),
    /// Writes this file with this text below the one folder the call's fence
    /// lets it write, as a reviewer leaves a test, and answers with this text
    Attach(&'static str, &'static str, &'static str),
    /// Links this file below that folder to a file outside it, and answers
    /// with this text
    AttachLinked(&'static str, &'static str),
    /// Answers like [`Self::Text`], with this cost for the session
    Billed(&'static str, Cost),
    /// Answers with this final message
    Say(&'static str),
    /// Answers with this final message, its session's context this many tokens
    SayAt(&'static str, u64),
    /// Blocks until the test releases it, then answers. Ended first, by
    /// the call's ending or by [`FakeClaude::stop`], it fails as the real
    /// adapters do.
    Hold(Hold),
    /// Blocks until the test releases it, then fails with this error
    HoldThenFail(Hold, AgentError),
}

/// What is queued for one role's calls
type RoleScript = (Role, VecDeque<Scripted>);

/// A call in flight that a test lets go of when it chooses
#[derive(Debug, Clone, Default)]
pub(crate) struct Hold(Arc<(Mutex<Held>, Condvar)>);

#[derive(Debug, Default)]
struct Held {
    entered: bool,
    released: bool,
    returned: bool,
}

impl Hold {
    /// Waits up to `within` for the call to begin, and says whether it did
    pub(crate) fn entered(&self, within: Duration) -> bool {
        let (held, changed) = &*self.0;
        let held = held.lock().unwrap();
        let (held, _) = changed
            .wait_timeout_while(held, within, |h| !h.entered)
            .unwrap();
        held.entered
    }

    /// Lets the call answer
    pub(crate) fn release(&self) {
        let (held, changed) = &*self.0;
        held.lock().unwrap().released = true;
        changed.notify_all();
    }

    /// Whether the call has answered
    pub(crate) fn returned(&self) -> bool {
        self.0.0.lock().unwrap().returned
    }

    /// Waits up to `within` for the call to answer, and says whether it did
    pub(crate) fn answered(&self, within: Duration) -> bool {
        let (held, changed) = &*self.0;
        let held = held.lock().unwrap();
        let (held, _) = changed
            .wait_timeout_while(held, within, |h| !h.returned)
            .unwrap();
        held.returned
    }

    pub(crate) fn block(&self) {
        self.block_unless(|| false);
    }

    /// Blocks until the test releases it, or until `ended` says the call
    /// was ended first, and says whether it was released
    pub(crate) fn block_unless(&self, ended: impl Fn() -> bool) -> bool {
        let (held, changed) = &*self.0;
        let mut held = held.lock().unwrap();
        held.entered = true;
        changed.notify_all();
        while !held.released && !ended() {
            held = changed.wait_timeout(held, LOOK_EVERY).unwrap().0;
        }
        held.returned = true;
        changed.notify_all();
        held.released
    }
}

/// A call as the stand-in Claude saw it
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    /// The call
    pub(crate) call: AgentCall,
    /// The settings file it named, as it stood during the call
    pub(crate) settings: serde_json::Value,
    /// The sandbox runtime's settings for its fence, as the real adapter writes them
    pub(crate) sandbox: serde_json::Value,
    /// Whether its build folder existed when the call started
    pub(crate) build_existed: bool,
}

/// Records every call and answers from a script, failing once it runs out
#[derive(Debug, Clone, Default)]
pub(crate) struct FakeClaude {
    seen: Arc<Mutex<Vec<Seen>>>,
    script: Arc<Mutex<VecDeque<Scripted>>>,
    // Answers kept for one role's calls, which no other role's call takes, so
    // calls that run at once answer the same however their threads are timed
    by_role: Arc<Mutex<Vec<RoleScript>>>,
    meter: Option<FakeMeter>,
    stopped: Arc<AtomicBool>,
    // What every call in flight last showed, as a transcript would say;
    // untracked until a test sets it
    active: Arc<Mutex<Option<CallActivity>>>,
}

impl FakeClaude {
    /// One whose calls spend what `meter` is told they do
    pub(crate) fn metered(meter: FakeMeter) -> Self {
        Self {
            meter: Some(meter),
            ..Self::default()
        }
    }

    /// The worker's own calls, in order: what every test before the review
    /// existed already asserted on, so a reviewer call never shows up and
    /// shifts their counts.
    pub(crate) fn calls(&self) -> Vec<AgentCall> {
        self.seen().into_iter().map(|s| s.call).collect()
    }

    /// The worker's own calls, with the settings file each one saw
    pub(crate) fn seen(&self) -> Vec<Seen> {
        self.all_seen()
            .into_iter()
            .filter(|s| s.call.role == Role::Worker)
            .collect()
    }

    /// Every call, worker and reviewer alike, in order
    pub(crate) fn all_calls(&self) -> Vec<AgentCall> {
        self.all_seen().into_iter().map(|s| s.call).collect()
    }

    /// Every call, with the settings file each one saw
    pub(crate) fn all_seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    /// Says what every call in flight last showed
    pub(crate) fn set_activity(&self, activity: CallActivity) {
        *self.active.lock().unwrap() = Some(activity);
    }

    /// Queues answers for its next calls, oldest first
    pub(crate) fn script(&self, steps: impl IntoIterator<Item = Scripted>) {
        self.script.lock().unwrap().extend(steps);
    }

    /// Queues answers for the next calls of `role`, which take them before
    /// any answer queued by [`FakeClaude::script`]
    pub(crate) fn script_for(&self, role: Role, steps: impl IntoIterator<Item = Scripted>) {
        let mut kept = self.by_role.lock().unwrap();
        match kept.iter_mut().find(|(r, _)| *r == role) {
            Some((_, queue)) => queue.extend(steps),
            None => kept.push((role, steps.into_iter().collect())),
        }
    }

    /// Ends every held call with [`AgentError::Stopped`], so a test's held
    /// calls return once it stops the runner
    pub(crate) fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    // Blocks on `hold` until it is released, or the call is ended first.
    fn hold(&self, hold: &Hold, call: &AgentCall, ending: &Ending) -> Result<(), AgentError> {
        let stopped = || self.stopped.load(Ordering::SeqCst);
        if hold.block_unless(|| stopped() || ending.asked()) {
            return Ok(());
        }
        Err(match stopped() {
            true => AgentError::Stopped,
            false => AgentError::TimedOut(call.harness.harness()),
        })
    }
}

impl Agents for FakeClaude {
    // The file Claude Code would be started with, so a test reads what a
    // call really left on disk.
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        crate::adapters::write_claude_settings(call, &[])
    }

    // The session as the terminal would resume it, with no sandbox around it.
    fn foreground(&self, call: &AgentCall) -> Result<std::process::Command, AgentError> {
        let mut command = std::process::Command::new("claude");
        command
            .arg("--settings")
            .arg(&call.settings)
            .args(["--resume", &call.session.id().0])
            .current_dir(&call.cwd);
        Ok(command)
    }

    // Beside the settings file, where a test can read what it would watch.
    fn transcript(&self, call: &AgentCall) -> Option<crate::ports::Transcript> {
        Some(crate::ports::Transcript {
            folder: call.settings.with_extension("transcripts"),
            threads: None,
        })
    }

    fn last_active(&self, _call: &AgentCall) -> CallActivity {
        self.active
            .lock()
            .unwrap()
            .unwrap_or(CallActivity::Untracked)
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        let settings: serde_json::Value = std::fs::read_to_string(&call.settings)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let build = settings["env"]["CARGO_TARGET_DIR"].as_str().map(Path::new);
        let build_existed = build.is_some_and(Path::is_dir);
        let sandbox = call
            .reach
            .fence
            .as_deref()
            .map_or(serde_json::Value::Null, |fence| {
                crate::adapters::srt_settings(&crate::adapters::fence_policy(fence))
            });
        self.seen.lock().unwrap().push(Seen {
            call: call.clone(),
            settings,
            sandbox,
            build_existed,
        });
        let own = {
            let mut kept = self.by_role.lock().unwrap();
            let queue = kept.iter_mut().find(|(r, _)| *r == call.role);
            queue.and_then(|(_, queue)| queue.pop_front())
        };
        let next = own.or_else(|| self.script.lock().unwrap().pop_front());
        let next = match next {
            Some(Scripted::Spend(account, usage, cost)) => {
                if let Some(meter) = &self.meter {
                    meter.set(account);
                }
                Some(Scripted::Reply(usage, cost))
            }
            other => other,
        };
        let said = |text: &str| answer(call, text);
        match next {
            Some(Scripted::Reply(usage, session_cost)) => Ok(AgentReply {
                usage,
                session_cost: Some(session_cost),
                ..said("done")
            }),
            Some(Scripted::Tokens(usage)) => Ok(AgentReply {
                usage,
                session_cost: None,
                ..said("done")
            }),
            Some(Scripted::Spend(..)) => unreachable!("turned into a reply above"),
            Some(Scripted::Fail(error)) => Err(error),
            Some(Scripted::Kill) => {
                std::fs::write(call.cwd.join(LEFT_BEHIND), "work in progress\n").unwrap();
                panic!("the runner is killed mid-turn");
            }
            Some(Scripted::Text(text) | Scripted::Say(text)) => Ok(said(text)),
            Some(Scripted::SayAt(text, context)) => Ok(AgentReply {
                context: Some(context),
                ..said(text)
            }),
            Some(Scripted::Billed(text, cost)) => Ok(AgentReply {
                session_cost: Some(cost),
                ..said(text)
            }),
            Some(Scripted::Hold(hold)) => {
                self.hold(&hold, call, ending)?;
                Ok(said("done"))
            }
            Some(Scripted::HoldThenFail(hold, error)) => {
                hold.block();
                Err(error)
            }
            Some(Scripted::Attach(file, text, reply)) => {
                write_in(&attach_folder(call), file, text);
                Ok(said(reply))
            }
            Some(Scripted::AttachLinked(file, reply)) => {
                let folder = attach_folder(call);
                let outside = folder.parent().unwrap().join("outside.rs");
                std::fs::write(&outside, "fn outside() {}\n").unwrap();
                let link = folder.join(file);
                std::fs::create_dir_all(link.parent().unwrap()).unwrap();
                std::os::unix::fs::symlink(&outside, link).unwrap();
                Ok(said(reply))
            }
            Some(Scripted::Plant(file, text)) => {
                write_in(&call.cwd, file, text);
                Ok(said("done"))
            }
            Some(Scripted::Stage(file, text)) => {
                write_in(&call.cwd, file, text);
                git(&call.cwd, &["add", file]);
                std::fs::remove_file(call.cwd.join(file)).unwrap();
                Ok(said("done"))
            }
            Some(Scripted::Commit(file, text)) => {
                write_in(&call.cwd, file, text);
                git(&call.cwd, &["add", file]);
                git(&call.cwd, &["commit", "--quiet", "-m", file]);
                Ok(said("committed"))
            }
            Some(Scripted::CommitFiles(files)) => {
                for (file, text) in files {
                    write_in(&call.cwd, file, text);
                    git(&call.cwd, &["add", file]);
                }
                git(&call.cwd, &["commit", "--quiet", "-m", "files"]);
                Ok(said("committed"))
            }
            Some(Scripted::Push(file, text)) => {
                push(&call.cwd, &[(file, text)], file);
                Ok(said("pushed"))
            }
            Some(Scripted::PushAt(file, text, context)) => {
                push(&call.cwd, &[(file, text)], file);
                Ok(AgentReply {
                    context: Some(context),
                    ..said("pushed")
                })
            }
            Some(Scripted::PushSay(file, text, say)) => {
                push(&call.cwd, &[(file, text)], file);
                Ok(said(say))
            }
            Some(Scripted::PushSayCompacted(file, text, say)) => {
                push(&call.cwd, &[(file, text)], file);
                // Its history now a summary, it knows the reply's line format
                // only if the hook handed the turn's prompt back.
                let remembered = after_compaction(&call.cwd, &settings_of(call));
                Ok(said(match remembered.contains("`thread <n>: fixed`") {
                    true => say,
                    false => "done",
                }))
            }
            Some(Scripted::MergeMain) => {
                git(&call.cwd, &["fetch", "--quiet", "origin", "main"]);
                git(
                    &call.cwd,
                    &[
                        "merge",
                        "--quiet",
                        "-X",
                        "theirs",
                        "--no-edit",
                        "origin/main",
                    ],
                );
                git(&call.cwd, &["push", "--quiet", "origin", "HEAD"]);
                Ok(said("merged"))
            }
            None => Err(AgentError::Failed(
                crate::settings::Harness::ClaudeCode,
                "the rig scripts no reply".into(),
            )),
        }
    }
}

// The one folder a call's fence lets it write.
fn attach_folder(call: &AgentCall) -> std::path::PathBuf {
    let fence = call.reach.fence.as_ref().expect("a fenced call");
    match fence.write.as_slice() {
        [folder] => folder.clone(),
        other => panic!("one folder to write: {other:?}"),
    }
}

// The settings file the call named, as it stands.
fn settings_of(call: &AgentCall) -> serde_json::Value {
    std::fs::read_to_string(&call.settings)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

// What Claude Code adds to a session it compacted: the output of each
// `SessionStart` hook whose matcher is `compact`, run in `cwd`.
fn after_compaction(cwd: &Path, settings: &serde_json::Value) -> String {
    let entries = settings["hooks"]["SessionStart"].as_array();
    let mut added = String::new();
    for entry in entries.into_iter().flatten() {
        if entry["matcher"] != "compact" {
            continue;
        }
        for hook in entry["hooks"].as_array().into_iter().flatten() {
            let command = hook["command"].as_str().expect("a hook has a command");
            let out = std::process::Command::new("sh")
                .args(["-c", command])
                .current_dir(cwd)
                .output()
                .expect("sh runs");
            added.push_str(&String::from_utf8_lossy(&out.stdout));
        }
    }
    added
}

// What a call that cost nothing answers, saying `text`.
fn answer(call: &AgentCall, text: &str) -> AgentReply {
    AgentReply {
        session_id: call.session.id().clone(),
        text: text.to_owned(),
        usage: Usage::default(),
        session_cost: Some(Cost(0)),
        context: None,
    }
}

// Writes each file, commits them in one commit with `message`, and pushes it
// the way a worker does.
fn push(cwd: &Path, files: &[(&str, &str)], message: &str) {
    for (file, text) in files {
        write_in(cwd, file, text);
        git(cwd, &["add", file]);
    }
    git(cwd, &["commit", "--quiet", "-m", message]);
    git(cwd, &["push", "--quiet", "origin", "HEAD"]);
}
