//! `kelpie --schema` and `--version`: shep's probes, answered for lookout
//!
//! `kelpie runner <project>`: a project's runner, run as a sheep
//! `kelpie`, started by shep as the adopted dog: the lease dog
//! `shep kelpie lease ...`: the maintainer's lease commands
//!
//! `shep kelpie add`, `start`, `pause`, `status`, `rule`, `rework`, `adopt`,
//! `gate`, `drop`, `timings`, `issue`, `attach`, `tell`, `pm`, `drain`, `undrain` and
//! `usage`: a project in the maintainer's own flock, the one `-p` names or
//! whose repo holds the folder it runs in. `usage` with no project reports
//! every project that has a usage ledger instead, and `usage --forge` reports
//! the forge ledger, which every project shares. `usage <project> --item`,
//! `--context` and `--reads` join a project's ledger to the transcripts of
//! its sessions.
//!
//! `shep kelpie watch [<issue> | pm] [--back <n>] [--full]`: the runner's
//! log and the transcripts of the sessions it names, followed live. Run as
//! `shep kelpie watch`.
//!
//! `shep kelpie doctor [<project>] [--test-alert]`: checks what the projects need
//! on this machine, and changes nothing. Run as `shep kelpie doctor`.
//!
//! `shep kelpie upgrade --ref <git ref> | --release <version> | --binary <path>`,
//! `shep kelpie upgrade --rollback`: installs a new kelpie over the one the
//! adopted dog runs and restarts the dog and each runner onto it, between
//! merges, or puts the previous build back. Each runner is handed over in place,
//! or drained first, so no call is cut short, unless `--now` is given. Run as
//! `shep kelpie upgrade ...`.
//!
//! `kelpie version [--json]`: the build's version and the shep version it is
//! made with, which `upgrade` reads from a build before it installs it.
//!
//! `kelpie confine <folder>... [--hooks=<json>]`: the hook that holds a worker's
//! file tools to its folders, and runs the project's `write` hooks. Claude Code
//! runs it; it is not for the maintainer. With
//! `--append <file>` it is the project manager's, which may only add to the
//! end of that file.
//!
//! `kelpie guard <git common dir> <worktree>`: the hook on every worker's
//! Bash calls that keeps the home folder's path and freeform pull request
//! titles out of what it publishes. Claude Code runs it, like `confine`.
//! On Codex it takes `--pin-folder`, and the command it answers with runs
//! it again with `--judge-here` when Codex runs the command in another folder.
//! With `--issues=<label>`, an `--agent=<name>` per implementer and
//! `--ledger=<file>` it is the issue writer's hook instead, which runs only
//! the commands it lists. With `--record` too it is the PostToolUse hook
//! that records what the session filed and read in that ledger. With `--after`
//! it is the PostToolUse hook that runs the project's `after_command` hooks.
//! With `--read-only` it is a Codex reviewer's, advisor's and retro's hook,
//! which runs only the commands that read the repo.
//! With `--pause=<build folder>` it is the PostToolUse hook that ends a
//! worker's turn at its next tool result while a pause is set.
//! With `--goose=<build folder>` it is a goose worker's PreToolUse hook: it
//! reads goose's own payload, judges a `shell` as a command and a `write` or
//! `edit` as `confine` would, and refuses any tool it does not list. With
//! `--read-only` as well it is a goose reviewer's, advisor's and retro's: a
//! `shell` runs only what reads, and `write`, `edit` and any other tool are refused.
//!
//! `shep kelpie github setup [--org <org>] [--name <app name>] [--replace]`:
//! registers kelpie's GitHub App for the maintainer's account or an
//! organization, and opens the page that installs it.
//!
//! `shep kelpie tools install`: installs the sandbox runtime every agent runs
//! in, the browser a worker sees the UI through, and the other tools, under
//! kelpie's home.
//!
//! `kelpie mcp-connect <socket>`: carries a worker's MCP stdio to the browser
//! kelpie runs outside its sandbox. The worker's harness starts it; it is not
//! for the maintainer.
//!
//! `shep kelpie totp [--rotate]`, run as `shep kelpie totp [--rotate]` where kelpie
//! is not on the PATH: prints the authenticator secret that answers a ruling
//! from ntfy, as a URI and a QR code to scan, drawing it the first time, or
//! afresh with `--rotate`. `shep kelpie totp --unlock` turns answers from
//! ntfy back on after too many wrong codes.

#![forbid(unsafe_code)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use shep_kelpie::confine::{APPEND, Verdict, judge_append, judge_with};
use shep_kelpie::guard::{self, Checkout};
use shep_kelpie::hooks::{Hooks, Role};
use shep_kelpie::tools::Tools;

/// A PreToolUse hook's exit code that refuses the tool call
const REFUSE: u8 = 2;

fn main() -> ExitCode {
    // First, before anything opens a descriptor or starts a thread: a build a
    // handover execs is asked here whether it reads the state, and takes it.
    shep_handover::record_launch_path();
    let arrival = match shep_handover::boot::<shep_kelpie::sheep::Carry>() {
        Ok(arrival) => arrival,
        Err(e) => {
            eprintln!("kelpie cannot take over from the runner that handed over: {e}");
            return ExitCode::FAILURE;
        }
    };
    shep_kelpie::schema::probe();
    let args = shep_kelpie::flock::verb_first(std::env::args().skip(1).collect());
    match args.as_slice() {
        [role, project] if role == "runner" => shep_kelpie::sheep::run(project, arrival),
        [command, rest @ ..] if command == "lease" => shep_kelpie::lease::cli::main(rest),
        [role, git_common_dir, worktree, local @ ..] if role == "guard" => {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let (issues, local) = guard::issue_rules(local);
            let (record, local) = guard::record_flag(&local);
            let (pin, local) = guard::pin_flag(&local);
            let (here, local) = guard::here_flag(&local);
            let (after, local) = guard::after_flag(&local);
            let (reading, local) = guard::reading_flag(&local);
            let (pause, local) = guard::pause_flag(&local);
            let (goose, local) = guard::goose_flag(&local);
            let (goose_write, local) = guard::goose_write_flag(&local);
            let (servers, local) = guard::goose_server_flags(&local);
            // `local` keeps the hooks' and the base's flags, which a pinned command
            // judges with again.
            let (base, named) = guard::take_base(&local);
            let (kelpie_pushes, named) = guard::kelpie_pushes_flag(&named);
            let checkout = Checkout {
                git_common_dir: Path::new(git_common_dir),
                worktree: Path::new(worktree),
                base: &base,
                kelpie_pushes,
            };
            let (hooks, folders) = match Hooks::take(&named) {
                Ok(taken) => taken,
                Err(why) => return hook(Verdict::Refuse(why)),
            };
            if reading {
                let mut input = Vec::new();
                if let Err(e) = std::io::stdin().lock().read_to_end(&mut input) {
                    return hook(Verdict::Refuse(format!(
                        "kelpie cannot read this tool call: {e}"
                    )));
                }
                // A goose call's read-only hook names its build folder too, and
                // reads goose's own payload.
                return hook(match (goose, goose_write) {
                    (Some(_), Some(folder)) => {
                        guard::judge_goose_writing(&input[..], &hooks, &servers, &folder)
                    }
                    (Some(_), None) => guard::judge_goose_reading(&input[..], &hooks, &servers),
                    (None, _) => guard::judge_reading(&input[..], &hooks),
                });
            }
            let paths = match guard::local_paths(home.as_deref(), &folders) {
                Ok(paths) => paths,
                Err(why) => return hook(Verdict::Refuse(why)),
            };
            let mut input = Vec::new();
            if let Err(e) = std::io::stdin().lock().read_to_end(&mut input) {
                return hook(Verdict::Refuse(format!(
                    "kelpie cannot read this tool call: {e}"
                )));
            }
            if let Some(build) = goose {
                // A goose worker's PreToolUse hook, in goose's own payload.
                return hook(guard::judge_goose(
                    &input[..],
                    home.as_deref(),
                    paths,
                    &hooks,
                    checkout,
                    &build,
                    &servers,
                ));
            }
            if let Some(build) = pause {
                // The PostToolUse hook that ends a turn at a pause.
                if let Some(answer) = guard::judge_pause(&build) {
                    println!("{answer}");
                }
                return ExitCode::SUCCESS;
            }
            if let Some(rules) = issues.as_ref().filter(|_| record) {
                guard::record_issues(&input[..], rules);
                return ExitCode::SUCCESS;
            }
            if after {
                let role = match issues {
                    Some(_) => Role::IssueWriter,
                    None => Role::Worker,
                };
                return hook(guard::judge_after(
                    &input[..],
                    &hooks,
                    role,
                    Path::new(worktree),
                ));
            }
            if let Some(rules) = issues {
                return hook(guard::judge_issues_with(
                    &input[..],
                    home.as_deref(),
                    paths,
                    &hooks,
                    &rules,
                ));
            }
            if here {
                let folder = std::env::current_dir()
                    .map_err(|e| format!("kelpie cannot tell which folder this runs in: {e}"));
                match folder.and_then(|f| guard::here(&input, &f)) {
                    Ok(moved) => input = moved,
                    Err(why) => return hook(Verdict::Refuse(why)),
                }
            }
            // The pinned command judges again by this program's full path,
            // never by a name the worker's PATH could answer.
            let again = || -> Result<Vec<String>, String> {
                let me = std::env::current_exe()
                    .ok()
                    .filter(|p| p.is_absolute())
                    .ok_or("kelpie cannot tell where its own program is")?;
                Ok([me.to_string_lossy().into_owned(), role.clone()]
                    .into_iter()
                    .chain([git_common_dir.clone(), worktree.clone()])
                    .chain(local.iter().cloned())
                    .chain([guard::HERE_FLAG.to_owned()])
                    .collect())
            };
            match guard::judge_with(&input[..], home.as_deref(), paths, &hooks, checkout) {
                Verdict::Allow if pin => match again()
                    .and_then(|again| guard::pinned(&input, Path::new(worktree), &again))
                {
                    Ok(answer) => {
                        println!("{answer}");
                        ExitCode::SUCCESS
                    }
                    Err(why) => hook(Verdict::Refuse(why)),
                },
                verdict => hook(verdict),
            }
        }
        [command, rest @ ..] if shep_kelpie::flock::VERBS.contains(&command.as_str()) => {
            shep_kelpie::flock::main(command, rest)
        }
        [command, rest @ ..] if command == "doctor" => shep_kelpie::doctor::main(rest),
        [command, rest @ ..] if command == "upgrade" => shep_kelpie::upgrade::main(rest),
        [command, rest @ ..] if command == "github" => shep_kelpie::github::setup::main(rest),
        [command] if command == "version" => version(false),
        [command, flag] if command == "version" && flag == "--json" => version(true),
        [role, flag, file] if role == "confine" && flag == APPEND => {
            hook(judge_append(std::io::stdin().lock(), Path::new(file)))
        }
        [role, folders @ ..] if role == "confine" && !folders.is_empty() => {
            // The project's `write` hooks ride on the command line after the folders.
            let (hooks, folders) = match Hooks::take(folders) {
                Ok(taken) => taken,
                Err(why) => return hook(Verdict::Refuse(why)),
            };
            let folders: Vec<PathBuf> = folders.iter().map(PathBuf::from).collect();
            hook(judge_with(std::io::stdin().lock(), &folders, &hooks))
        }
        [command, sub] if command == "tools" && sub == "install" => install_tools(),
        [command, socket] if command == "mcp-connect" => {
            match shep_kelpie::bridge::connect(Path::new(socket)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(why) => {
                    eprintln!("{why}");
                    ExitCode::FAILURE
                }
            }
        }
        [command] if command == "totp" => totp(false),
        [command, flag] if command == "totp" && flag == "--rotate" => totp(true),
        [command, flag] if command == "totp" && flag == "--unlock" => unlock(),
        // `shep kelpie` with no verb also sets SHEP_DOG_NAME; only the
        // shepherd's own start sets SHEP_NAME.
        [] if ["SHEP_DOG_NAME", "SHEP_NAME"]
            .iter()
            .all(|key| std::env::var_os(key).is_some()) =>
        {
            shep_kelpie::dog::run()
        }
        _ => {
            if let Some(gone) = shep_kelpie::settings::removed_command(&args) {
                eprintln!("{gone}");
                return ExitCode::from(2);
            }
            eprintln!(
                "usage: shep-kelpie add [<project>] | add <ticket>\n       shep-kelpie start | pause | finish | status\n       shep-kelpie rule [<id> <answer>]\n       shep-kelpie rework <pr> | adopt <pr>\n       shep-kelpie gate [<ticket>] | drop [<ticket>]\n       shep-kelpie doctor [<project>] [--test-alert]\n       shep-kelpie upgrade [--now] --ref <git ref> | --release <version> | --binary <path> | --rollback\n       shep-kelpie version [--json]\n       shep-kelpie runner <project>\n{}\n       shep-kelpie confine <folder>...\n       shep-kelpie guard <git common dir> <worktree>\n       shep-kelpie github setup [--org <org>] [--name <app name>] [--replace]\n       shep-kelpie tools install\n       shep-kelpie mcp-connect <socket>\n       shep-kelpie totp [--rotate | --unlock]\n\nAdopted as `kelpie`, the same verbs run as `shep kelpie <verb>`, and `--` reaches `lease run`.\n\n{}\n\n{}",
                shep_kelpie::lease::cli::USAGE,
                shep_kelpie::flock::USAGE,
                shep_kelpie::flock::rule::HELP
            );
            ExitCode::from(2)
        }
    }
}

fn version(json: bool) -> ExitCode {
    let build = shep_kelpie::home::Build::this();
    if json {
        let line = serde_json::to_string(&build).expect("a build serializes");
        println!("{line}");
    } else {
        println!("shep-kelpie {} (shep {})", build.kelpie, build.shep);
    }
    ExitCode::SUCCESS
}

// Kelpie's home as the runner works it out.
fn kelpie_home() -> Option<PathBuf> {
    shep_kelpie::home::kelpie_home()
        .inspect_err(|e| eprintln!("{e}"))
        .ok()
}

fn totp(rotate: bool) -> ExitCode {
    let Some(home) = kelpie_home() else {
        return ExitCode::FAILURE;
    };
    match shep_kelpie::totp::show(&home.join("totp/secret"), rotate) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn unlock() -> ExitCode {
    let Some(home) = kelpie_home() else {
        return ExitCode::FAILURE;
    };
    match shep_kelpie::totp::answers::Answers::in_folder(home.join("totp")).unlock() {
        Ok(()) => {
            println!("answers from ntfy are on again");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cannot turn answers from ntfy back on: {e}");
            ExitCode::FAILURE
        }
    }
}

fn install_tools() -> ExitCode {
    let Some(home) = kelpie_home() else {
        return ExitCode::FAILURE;
    };
    let tools = Tools::under(&home);
    match tools.install() {
        Ok(()) => {
            println!("installed kelpie's tools in {}", tools.dir().display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

// A PreToolUse hook's answer: a refusal's reason goes to Claude on stderr.
fn hook(verdict: Verdict) -> ExitCode {
    match verdict {
        Verdict::Allow => ExitCode::SUCCESS,
        Verdict::Refuse(why) => {
            eprintln!("{why}");
            ExitCode::from(REFUSE)
        }
    }
}
