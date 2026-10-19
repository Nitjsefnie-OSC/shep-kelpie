//! The sandbox a Claude Code call runs in, whole
//!
//! Claude Code, its tools, hooks and MCP servers all run inside kelpie's
//! sandbox. Besides what the call's fence allows, it writes its transcript
//! folder and a scratch folder, and reaches the model's endpoint. Its own
//! sandbox stays off: on macOS a sandbox cannot start inside another.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{ClaudeCli, LOGIN_FIX, LOGIN_VAR, argv, terminal_argv};
use crate::fence;
use crate::ports::{AgentCall, AgentError, Fence, Policy};
use crate::profile::CREDENTIALS;

impl ClaudeCli {
    // The call's command inside its sandbox.
    pub(super) fn sandboxed_command(&self, call: &AgentCall) -> Result<Command, AgentError> {
        let mut command = self.command(call, argv(call))?;
        // A headless turn that ends is over, so nothing it waits on in
        // the background can ever wake it.
        command.env("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS", "1");
        self.inside(call, &command)
    }

    // The call's session for the maintainer's terminal, inside the same sandbox.
    pub(super) fn terminal_command(&self, call: &AgentCall) -> Result<Command, AgentError> {
        let command = self.command(call, terminal_argv(call))?;
        self.inside(call, &command)
    }

    // `claude` with `argv`, its scratch folder emptied and its transcript
    // folder made. A scratch folder swapped for a link is removed, not followed.
    fn command(&self, call: &AgentCall, argv: Vec<OsString>) -> Result<Command, AgentError> {
        // `claude` would start and fail with its own "not logged in", which a ruling
        // would carry, in place of the fix.
        if call.reach.block_keychain && self.macos && self.login.is_none() {
            return Err(AgentError::Setup(format!(
                "the keychain is blocked in this repo's calls and {LOGIN_VAR} is not set in the \
                 runner's environment, so claude has no login: {LOGIN_FIX}"
            )));
        }
        let scratch = scratch(call);
        let setup = |what: &Path, e: std::io::Error| {
            AgentError::Setup(format!("cannot make {}: {}", what.display(), e.kind()))
        };
        match std::fs::remove_dir_all(&scratch) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(setup(&scratch, e)),
            _ => {}
        }
        for folder in [&scratch, &transcripts(&self.home, &call.cwd)?] {
            std::fs::create_dir_all(folder).map_err(|e| setup(folder, e))?;
        }
        // A sheep has no `TMPDIR`, so a shell falls back to `/tmp`, which the
        // sandbox refuses, and a heredoc cannot make its temp file there. zsh
        // ignores `TMPDIR` for a heredoc and writes under `TMPPREFIX`, a prefix
        // that zsh appends to, so it gets one in the same folder. Only
        // `claude` is given them, through `env`: `srt` puts its proxy socket
        // under its own `TMPDIR`, where a path past 104 bytes keeps it from
        // starting and a folder the call writes lets the call delete the socket.
        let mut temp = OsString::from("TMPDIR=");
        temp.push(&scratch);
        let mut prefix = OsString::from("TMPPREFIX=");
        prefix.push(scratch.join("zsh"));
        let mut command = crate::spawn::command("/usr/bin/env");
        command
            .arg(temp)
            .arg(prefix)
            .arg(&self.program)
            .args(argv)
            .current_dir(&call.cwd)
            .env("CLAUDE_CODE_TMPDIR", &scratch)
            .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1");
        if let Some(login) = &self.login {
            command.env(LOGIN_VAR, login.expose());
        }
        Ok(command)
    }

    fn inside(&self, call: &AgentCall, command: &Command) -> Result<Command, AgentError> {
        let policy = policy(call, &self.home, &self.unfenced_no_read)?;
        self.sandbox
            .wrap(&policy, &sandbox_settings(call), command)
            .map_err(|e| AgentError::Setup(e.to_string()))
    }
}

/// This harness, as the fence names it
const HARNESS: &str = "Claude Code";

/// The model's endpoint, the one host every call reaches
///
/// The worker's commands reach it too, since they share the sandbox, so
/// `kelpie guard` refuses an agent started from them. Not
/// `platform.claude.com`, where a login is refreshed: the sandbox cannot
/// write the keychain, so a refresh there would be lost.
const MODEL_HOST: &str = "api.anthropic.com";

// Claude Code names a working folder's transcript folder this way, and
// hashes past this length. Kelpie refuses a folder it cannot name.
const LONGEST_NAME: usize = 200;

/// What a fenced session may do, from its fence alone
pub(crate) fn fence_policy(fence: &Fence) -> Policy {
    Policy {
        write: fence.write.clone(),
        no_write: fence.no_write.clone(),
        no_read: fence.no_read.clone(),
        read: fence.read.clone(),
        open: Vec::new(),
        scratch: Vec::new(),
        hosts: fence.hosts.clone(),
        sockets: fence.sockets.clone(),
        forward: None,
        denied_hosts: Vec::new(),
        denied_addresses: Vec::new(),
        // Without it, `gh` fails TLS verification on macOS: x509 OSStatus -26276.
        verify_tls: true,
        no_keychain: false,
        listen: false,
        services: Vec::new(),
    }
}

/// What a call is held to before its harness adds its own needs: its fence's
/// policy, or with no fence the credentials and `unfenced` unread
///
/// Every harness starts here, so a call with no fence is denied `unfenced`
/// whichever one runs it. Its own folder stays readable, since a worktree
/// can sit inside the shepherd's home.
pub(crate) fn call_policy(call: &AgentCall, unfenced: &[String]) -> Policy {
    match &call.reach.fence {
        Some(fence) => fence_policy(fence),
        None => {
            let no_read = CREDENTIALS.iter().map(|&p| p.to_owned());
            let mut policy = Policy {
                no_read: no_read.chain(unfenced.iter().cloned()).collect(),
                ..Policy::default()
            };
            if !unfenced.is_empty() {
                policy.read.push(call.cwd.clone());
            }
            policy
        }
    }
}

/// The whole call's policy: its fence, or none, and what Claude Code itself needs
///
/// A call with no fence may not read `unfenced` either, its own folder apart.
///
/// # Errors
///
/// [`AgentError::Setup`] when the call's transcript folder cannot be named.
pub(crate) fn policy(
    call: &AgentCall,
    home: &Path,
    unfenced: &[String],
) -> Result<Policy, AgentError> {
    let mut policy = call_policy(call, unfenced);
    policy.no_read.extend(fence::others_credentials(HARNESS));
    policy.no_keychain = call.reach.block_keychain;
    let transcripts = transcripts(home, &call.cwd)?;
    // Auto memory there loads into every session in the folder, a review's too.
    policy.no_write.push(transcripts.join("memory"));
    policy.write.extend([transcripts, scratch(call)]);
    // Claude Code reads its own files for the call wherever they are kept.
    policy.read.extend(
        [&call.settings, &scratch(call)]
            .into_iter()
            .chain(&call.instructions)
            .chain(&call.plugin_dirs)
            .chain(&call.reach.read)
            .cloned(),
    );
    // The hook that hands the prompt back after a compaction reads it inside the sandbox.
    if call.compact_window.is_some() {
        policy
            .read
            .push(super::settings::prompt_file(&call.settings));
    }
    // The servers run inside the call's sandbox, and the code graph keeps its index in its own folder.
    if crate::adapters::mcp::loads(call) {
        policy.read.push(super::mcp::mcp_file(&call.settings));
    }
    crate::adapters::mcp::fence_the_graph(&mut policy, call);
    crate::adapters::mcp::fence_the_browser(&mut policy, call);
    policy.hosts.push(MODEL_HOST.to_owned());
    Ok(policy)
}

/// The folder Claude Code keeps the call's transcripts in
///
/// # Errors
///
/// [`AgentError::Setup`] when the working folder's name is past Claude Code's limit.
pub(crate) fn transcripts(home: &Path, cwd: &Path) -> Result<PathBuf, AgentError> {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_owned());
    // One `-` per UTF-16 unit, as Claude Code's JavaScript replaces them.
    let name: String = cwd
        .to_string_lossy()
        .chars()
        .flat_map(|c| {
            let kept = c.is_ascii_alphanumeric().then_some(c);
            std::iter::repeat_n(kept.unwrap_or('-'), kept.map_or(c.len_utf16(), |_| 1))
        })
        .collect();
    if name.len() > LONGEST_NAME {
        return Err(AgentError::Setup(format!(
            "{} is too long a path for kelpie to find its transcripts",
            cwd.display()
        )));
    }
    Ok(home.join(".claude").join("projects").join(name))
}

/// The call's scratch folder, beside its settings file, for Claude Code's temporary files
pub(crate) fn scratch(call: &AgentCall) -> PathBuf {
    call.settings.with_extension("tmp")
}

/// Where the sandbox's own settings for the call go, beside Claude Code's
pub(crate) fn sandbox_settings(call: &AgentCall) -> PathBuf {
    call.settings.with_extension("sandbox.json")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::*;
    use crate::adapters::{ClaudeCli, LoginToken, SandboxRuntime};
    use crate::ports::Ending;
    use crate::ports::{Agents, Role, Session, SessionId, Tools};
    use crate::profile::WorkerProfile;
    use crate::settings::Effort;
    use crate::test::OpenSandbox;
    use crate::tools::Tools as KelpieTools;

    const RESULT: &str = include_str!("../../../fixtures/claude-p-result.json");

    struct World {
        dir: tempfile::TempDir,
    }

    impl World {
        fn new() -> Self {
            let w = Self {
                dir: crate::test::short_tempdir(),
            };
            for folder in ["home", "wt", "build", "worker"] {
                std::fs::create_dir_all(w.path(folder)).unwrap();
            }
            std::fs::write(w.path("result.json"), RESULT).unwrap();
            let script = format!(
                "#!/bin/sh\ntouch {}\ncat {}\n",
                w.path("ran").display(),
                w.path("result.json").display()
            );
            crate::test::write_script(&w.path("claude"), &script);
            w
        }

        fn path(&self, p: &str) -> PathBuf {
            self.dir.path().canonicalize().unwrap().join(p)
        }

        fn call(&self, role: Role) -> AgentCall {
            AgentCall {
                issue: 7,
                settings: self.path("worker/settings.json"),
                ..AgentCall::new(
                    role,
                    Tools::Answer,
                    crate::settings::AgentHarness::ClaudeCode,
                    "claude-sonnet-5",
                    Effort::Medium,
                    Session::New(SessionId("s".into())),
                    self.path("wt"),
                    "go",
                )
            }
        }

        fn worker(&self) -> AgentCall {
            let (wt, build) = (self.path("wt"), self.path("build"));
            let profile = WorkerProfile {
                worktree: &wt,
                build: &build,
                git_common_dir: Path::new("/k/repo/.git"),
                git_dir: Path::new("/k/repo/.git/worktrees/7"),
                branch: "kelpie/7",
                kelpie: Path::new("/k/kelpie"),
                kelpie_home: Path::new("/k"),
                repo: Path::new("/k/repo"),
                guard_hooks: &[],
                allowed_domains: &[],
                build_env: &BTreeMap::new(),
                shep_home: Path::new("/k/shep"),
                door: Path::new("/k/dog/lease.sock"),
                token: None,
            };
            AgentCall {
                tools: Tools::Work,
                agent_tools: None,
                reach: profile.reach(),
                ..self.call(Role::Worker)
            }
        }

        // The shepherd's home, which the runner denies every call with no fence.
        fn shep_unread(&self) -> String {
            format!("{}/**", self.path("shep").display())
        }

        fn cli(&self, sandbox: Arc<dyn crate::ports::Sandbox>) -> ClaudeCli {
            ClaudeCli::with_program(self.path("claude"))
                .unfenced_unread(&self.path("shep"))
                .sandboxed(sandbox, self.path("home"))
        }
    }

    #[test]
    fn every_role_runs_inside_the_sandbox_with_its_settings_beside_claudes() {
        let w = World::new();
        let sandbox = OpenSandbox::default();
        let cli = w.cli(Arc::new(sandbox.clone()));
        for call in [w.worker(), w.call(Role::Reviewer)] {
            cli.prepare(&call).unwrap();
            cli.run(&call, &Ending::default()).unwrap();
        }
        let wrapped = sandbox.wrapped();
        assert_eq!(wrapped.len(), 2);
        for (_, settings) in &wrapped {
            assert_eq!(settings, &w.path("worker/settings.sandbox.json"));
        }
    }

    // Without the deny in `World::cli`, no test of a call's sandbox would see
    // the shepherd's home read.
    #[test]
    fn the_test_cli_denies_an_unfenced_call_the_shepherds_home() {
        let w = World::new();
        let sandbox = OpenSandbox::default();
        let cli = w.cli(Arc::new(sandbox.clone()));
        let call = w.call(Role::Reviewer);
        cli.prepare(&call).unwrap();
        cli.run(&call, &Ending::default()).unwrap();
        let wrapped = sandbox.wrapped();
        let [(open, _)] = wrapped.as_slice() else {
            panic!("wrapped once, not {wrapped:?}")
        };
        assert!(
            open.no_read.contains(&w.shep_unread()),
            "{:?}",
            open.no_read
        );
    }

    #[test]
    fn a_missing_sandbox_runs_nothing() {
        let w = World::new();
        let none = SandboxRuntime::new(KelpieTools::at(w.path("no-tools")));
        let cli = w.cli(Arc::new(none));
        for call in [w.worker(), w.call(Role::Reviewer)] {
            let err = cli.run(&call, &Ending::default()).unwrap_err();
            assert!(
                matches!(&err, AgentError::Setup(why) if why.contains("tools install")),
                "{err:?}"
            );
        }
        assert!(!w.path("ran").exists(), "claude ran without its sandbox");
    }

    #[test]
    fn a_worker_writes_its_fence_its_transcripts_and_its_scratch_and_reaches_the_model() {
        let w = World::new();
        let policy = policy(&w.worker(), &w.path("home"), &[]).unwrap();
        let transcripts = transcripts(&w.path("home"), &w.path("wt")).unwrap();
        let scratch = w.path("worker/settings.tmp");
        assert_eq!(policy.write[..2], [w.path("wt"), w.path("build")]);
        assert_eq!(
            policy.write[policy.write.len() - 2..],
            [transcripts.clone(), scratch.clone()]
        );
        assert!(policy.no_write.contains(&transcripts.join("memory")));
        assert!(policy.no_write.contains(&w.path("wt/.claude")));
        assert!(policy.no_write.contains(&w.path("wt/**/.codex")));
        assert_eq!(policy.hosts[..2], ["github.com", "api.github.com"]);
        assert_eq!(policy.hosts.last().unwrap(), MODEL_HOST);
        assert!(policy.read.contains(&w.path("worker/settings.json")));
        assert!(policy.no_read.iter().any(|p| p == "~/.ssh/**"));
        // Another harness's login, and none of Claude Code's own, which it reads to run.
        assert!(policy.no_read.iter().any(|p| p == "~/.codex/**"));
        assert!(!policy.no_read.iter().any(|p| p.starts_with("~/.claude")));
        assert!(policy.verify_tls);
    }

    #[test]
    fn a_retro_reads_the_plugin_folder_its_skill_is_in() {
        let w = World::new();
        let call = AgentCall {
            role: Role::Pm,
            tools: Tools::Retro,
            plugin_dirs: vec![w.path("skills/mattpocock")],
            ..w.worker()
        };
        let policy = policy(&call, &w.path("home"), &[]).unwrap();
        assert!(policy.read.contains(&w.path("skills/mattpocock")));
    }

    // The hook that hands a compacted turn's prompt back runs `cat` on it
    // inside the sandbox, which denies the whole of kelpie's home but for
    // what the call reads.
    #[test]
    fn a_worker_with_a_compact_window_reads_back_the_prompt_its_hook_prints() {
        let w = World::new();
        let mut call = w.worker();
        let without = policy(&call, &w.path("home"), &[]).unwrap();
        assert!(!without.read.contains(&w.path("worker/settings.prompt.md")));
        call.compact_window = std::num::NonZeroU32::new(200_000);
        let with = policy(&call, &w.path("home"), &[]).unwrap();
        assert!(with.read.contains(&w.path("worker/settings.prompt.md")));
    }

    #[test]
    fn a_call_on_a_covered_repo_runs_without_the_keychain_and_any_other_with_it() {
        let w = World::new();
        for mut call in [w.worker(), w.call(Role::Reviewer)] {
            let home = w.path("home");
            assert!(!policy(&call, &home, &[]).unwrap().no_keychain);
            call.reach.block_keychain = true;
            assert!(policy(&call, &home, &[]).unwrap().no_keychain);
        }
    }

    #[test]
    fn a_review_writes_nothing_of_the_worktrees_and_reaches_only_the_model() {
        let w = World::new();
        let mut review = w.call(Role::Reviewer);
        review.settings = w.path("worker/review-settings.json");
        review.reach.read = vec![w.path("extra")];
        let policy = policy(&review, &w.path("home"), &[]).unwrap();
        assert_eq!(
            policy.write,
            [
                transcripts(&w.path("home"), &w.path("wt")).unwrap(),
                w.path("worker/review-settings.tmp"),
            ]
        );
        assert_eq!(policy.hosts, [MODEL_HOST]);
        let mut denied: Vec<String> = CREDENTIALS.map(str::to_owned).into();
        denied.extend([
            "~/.codex/**".into(),
            "~/.pi/**".into(),
            "~/.config/goose/**".into(),
            "~/.local/share/goose/**".into(),
            "~/.local/state/goose/**".into(),
            "~/Library/Application Support/Block/goose/**".into(),
        ]);
        assert_eq!(policy.no_read, denied);
        assert!(policy.read.contains(&w.path("extra")));
        assert!(!policy.verify_tls);
    }

    // The shepherd's home holds the sheep entry's `env`, a gateway's key with
    // it, and `dogs.toml` the webhook's URL. The worktree under it stays readable.
    #[test]
    fn an_unfenced_call_reads_nothing_of_the_shepherds_home_but_its_own_folder() {
        let w = World::new();
        let review = w.call(Role::Reviewer);
        let shep = vec![format!("{}/**", w.path("").display())];
        let policy = policy(&review, &w.path("home"), &shep).unwrap();
        assert!(policy.no_read.contains(&shep[0]), "{:?}", policy.no_read);
        assert!(policy.read.contains(&w.path("wt")));
        let deny = super::super::settings::unfenced_reads(&review.cwd, &review.reach, &shep);
        let deny = deny.join(" ");
        let worker = format!("Read(/{}/**)", w.path("worker").display());
        assert!(deny.contains(&worker), "{deny}");
        assert!(
            !deny.contains(&w.path("wt").display().to_string()),
            "{deny}"
        );
        let without = super::policy(&review, &w.path("home"), &[]).unwrap();
        assert!(
            !without.read.contains(&w.path("wt")),
            "nothing to read around"
        );
    }

    #[test]
    fn no_role_can_start_a_background_command_or_agent() {
        let w = World::new();
        let cli = w.cli(Arc::new(OpenSandbox::default()));
        for call in [w.worker(), w.call(Role::Reviewer)] {
            let command = cli.sandboxed_command(&call).unwrap();
            let set = command
                .get_envs()
                .find(|(name, _)| *name == "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS");
            assert_eq!(
                set.and_then(|(_, value)| value),
                Some(std::ffi::OsStr::new("1")),
                "{:?} can still run a task in the background",
                call.role
            );
        }
    }

    // A shell's heredoc makes a temp file under `TMPDIR`, or `/tmp` with none,
    // and the sandbox refuses a write there. `srt` makes its own proxy socket
    // under its `TMPDIR`: past 104 bytes it cannot start, and in a folder the
    // call writes the call could delete the socket. So only the command
    // inside `srt` gets it, as `env TMPDIR=<folder> TMPPREFIX=<folder>/zsh
    // claude ...`. zsh ignores `TMPDIR` for a heredoc and writes under
    // `TMPPREFIX`, `/tmp/zsh` by default, which the sandbox refuses.
    #[test]
    fn every_role_gets_a_temp_folder_its_sandbox_can_write_and_srt_keeps_its_own() {
        let w = World::new();
        let cli = w.cli(Arc::new(OpenSandbox::default()));
        let issue_writer = w.call(Role::IssueWriter);
        for call in [w.worker(), w.call(Role::Reviewer), issue_writer] {
            let command = cli.sandboxed_command(&call).unwrap();
            let policy = policy(&call, &w.path("home"), &[]).unwrap();
            let role = call.role;
            let env = |name: &str| command.get_envs().find(|(n, _)| *n == name);
            assert!(env("TMPDIR").is_none(), "{role:?}: srt would see TMPDIR");
            let claude_tmp = env("CLAUDE_CODE_TMPDIR").and_then(|(_, value)| value);
            let claude_tmp = PathBuf::from(claude_tmp.expect("CLAUDE_CODE_TMPDIR"));
            assert!(
                policy.write.contains(&claude_tmp),
                "{role:?}: {claude_tmp:?}"
            );
            assert_eq!(command.get_program(), "/usr/bin/env");
            let mut words = command.get_args();
            let temp = words.next().unwrap().to_str().unwrap();
            let folder = PathBuf::from(temp.strip_prefix("TMPDIR=").expect(temp));
            assert!(
                policy.write.contains(&folder),
                "{role:?}: TMPDIR {folder:?} is not in {:?}",
                policy.write
            );
            // zsh ignores `TMPDIR` for a heredoc and writes under `TMPPREFIX`.
            let prefix = words.next().unwrap().to_str().unwrap();
            let prefix = PathBuf::from(prefix.strip_prefix("TMPPREFIX=").expect(prefix));
            assert_eq!(prefix, folder.join("zsh"), "{role:?}");
            assert_eq!(words.next().unwrap(), w.path("claude").as_os_str());
        }
    }

    type Cmd = fn(&ClaudeCli, &AgentCall) -> Result<Command, AgentError>;

    // With the keychain blocked, a call's login is the token the maintainer
    // made with `claude setup-token`. It is a variable of the command inside
    // the sandbox and never an argument, which `ps` shows.
    #[test]
    fn every_role_s_claude_gets_the_login_token_and_none_without_one() {
        let w = World::new();
        let token = "sk-ant-oat01-for-the-test";
        let with = w
            .cli(Arc::new(OpenSandbox::default()))
            .with_login(LoginToken::new(token.to_owned()));
        let without = w.cli(Arc::new(OpenSandbox::default()));
        assert!(
            !format!("{with:?}").contains(token),
            "Debug shows the token"
        );
        let issue_writer = w.call(Role::IssueWriter);
        for call in [w.worker(), w.call(Role::Reviewer), issue_writer] {
            let role = call.role;
            let login = |cli: &ClaudeCli, command: Cmd| {
                let command = command(cli, &call).unwrap();
                let set = command.get_envs().find(|(n, _)| *n == LOGIN_VAR);
                let args: Vec<_> = command.get_args().collect();
                assert!(!args.iter().any(|a| a.to_string_lossy().contains(token)));
                set.map(|(_, value)| value.map(|v| v.to_string_lossy().into_owned()))
            };
            let session = ClaudeCli::sandboxed_command;
            assert_eq!(
                login(&with, session),
                Some(Some(token.to_owned())),
                "{role:?}"
            );
            assert_eq!(login(&without, session), None, "{role:?}");
            let terminal = ClaudeCli::terminal_command;
            assert_eq!(
                login(&with, terminal),
                Some(Some(token.to_owned())),
                "{role:?}"
            );
        }
    }

    // With the keychain blocked and no token, `claude` would start and fail
    // with its own "not logged in", which a ruling would carry. The call says
    // what doctor says instead, and runs nothing.
    #[test]
    fn a_blocked_keychain_and_no_login_token_refuses_the_call_with_the_fix() {
        let w = World::new();
        let token = LoginToken::new("sk-ant-oat01-for-the-test".to_owned());
        let cli = |login: Option<LoginToken>, macos: bool| {
            w.cli(Arc::new(OpenSandbox::default()))
                .with_login(login)
                .on_macos(macos)
        };
        let blocked = |mut call: AgentCall| {
            call.reach.block_keychain = true;
            call
        };
        for call in [w.worker(), w.call(Role::Reviewer)] {
            let call = blocked(call);
            for run in [ClaudeCli::sandboxed_command, ClaudeCli::terminal_command] {
                let err = run(&cli(None, true), &call).unwrap_err();
                let AgentError::Setup(why) = &err else {
                    panic!("{err:?}")
                };
                assert!(why.contains("`claude setup-token`"), "{why}");
                assert!(
                    why.contains("`shep secret set CLAUDE_CODE_OAUTH_TOKEN`"),
                    "{why}"
                );
                run(&cli(token.clone(), true), &call).unwrap();
                // No keychain off macOS, and a call that keeps it logs in there.
                run(&cli(None, false), &call).unwrap();
                run(&cli(None, true), &w.worker()).unwrap();
            }
            let err = cli(None, true).run(&call, &Ending::default()).unwrap_err();
            assert!(matches!(err, AgentError::Setup(_)), "{err:?}");
        }
        assert!(!w.path("ran").exists(), "claude ran with no login");
    }

    #[test]
    fn the_scratch_folder_is_emptied_and_a_link_there_is_not_followed() {
        let w = World::new();
        let cli = w.cli(Arc::new(OpenSandbox::default()));
        let call = w.worker();
        std::fs::create_dir_all(w.path("elsewhere")).unwrap();
        std::fs::write(w.path("elsewhere/keep"), "").unwrap();
        std::os::unix::fs::symlink(w.path("elsewhere"), scratch(&call)).unwrap();
        cli.run(&call, &Ending::default()).unwrap();
        assert!(w.path("elsewhere/keep").exists());
        assert!(scratch(&call).is_dir() && !scratch(&call).is_symlink());
        assert!(
            transcripts(&w.path("home"), &w.path("wt"))
                .unwrap()
                .is_dir()
        );
    }

    #[test]
    fn a_session_for_the_terminal_resumes_in_the_worker_s_sandbox_and_asks_as_usual() {
        let w = World::new();
        let sandbox = OpenSandbox::default();
        let cli = w.cli(Arc::new(sandbox.clone()));
        let call = AgentCall {
            session: Session::Resume(SessionId("s".into())),
            prompt: String::new(),
            ..w.worker()
        };
        cli.prepare(&call).unwrap();
        let command = cli.foreground(&call).unwrap();
        let args: Vec<&str> = command.get_args().map(|a| a.to_str().unwrap()).collect();
        let settings = w.path("worker/settings.json");
        let temp = format!("TMPDIR={}", w.path("worker/settings.tmp").display());
        let prefix = format!("TMPPREFIX={}", w.path("worker/settings.tmp/zsh").display());
        let claude = w.path("claude");
        assert_eq!(command.get_program(), "/usr/bin/env");
        assert_eq!(
            args,
            [
                temp.as_str(),
                prefix.as_str(),
                claude.to_str().unwrap(),
                "--model",
                "claude-sonnet-5",
                "--effort",
                "medium",
                "--setting-sources",
                "project",
                "--settings",
                settings.to_str().unwrap(),
                "--tools",
                "Bash,Read,Edit,Write,Glob,Grep,Skill",
                "--strict-mcp-config",
                "--resume",
                "s",
            ]
        );
        assert_eq!(command.get_current_dir(), Some(w.path("wt").as_path()));
        // Only those it sets: it also unsets whatever `spawn` hides.
        let set = command.get_envs().filter(|(_, value)| value.is_some());
        let env: Vec<_> = set.map(|(name, _)| name).collect();
        assert_eq!(
            env,
            ["CLAUDE_CODE_DISABLE_AUTO_MEMORY", "CLAUDE_CODE_TMPDIR"]
        );
        let wrapped = sandbox.wrapped();
        let [(policy_used, file)] = wrapped.as_slice() else {
            panic!("wrapped once, not {wrapped:?}")
        };
        assert_eq!(policy_used, &policy(&call, &w.path("home"), &[]).unwrap());
        assert_eq!(file, &w.path("worker/settings.sandbox.json"));
        assert!(
            !w.path("ran").exists(),
            "nothing runs until the terminal does"
        );
    }

    #[test]
    fn the_transcript_folder_is_named_as_claude_code_names_it() {
        let home = Path::new("/Users/me");
        assert_eq!(
            transcripts(home, Path::new("/Users/me/.kelpie/wt/koji/37")).unwrap(),
            Path::new("/Users/me/.claude/projects/-Users-me--kelpie-wt-koji-37")
        );
        // An emoji is two UTF-16 units, so two dashes.
        assert_eq!(
            transcripts(home, Path::new("/x/é🐑")).unwrap(),
            Path::new("/Users/me/.claude/projects/-x----")
        );
        let long = format!("/{}", "a".repeat(200));
        assert!(matches!(
            transcripts(home, Path::new(&long)),
            Err(AgentError::Setup(_))
        ));
    }
}
