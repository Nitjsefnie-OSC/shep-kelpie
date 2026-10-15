//! `shep kelpie <verb>`: the command line, read into a trigger for a project

use std::ffi::OsStr;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::control::Format;
use super::{Checkout, Launch, add, attach, control, pm, rule, set, usage, watch};
use crate::adapters::{ClaudeCli, Gh, LoginToken, SystemGit};
use crate::agents::Agents;
use crate::issues::{self, Mode, Project, Writer};
use crate::ports::SandboxError;
use crate::runner::{ProjectName, ProjectPaths};
use crate::settings::source;
use crate::shepherd::{self, Connection, Dial, Socket};
use crate::state::Projects;
use crate::tools::Tools;
use crate::upgrade::restart::{Interrupt, Patience};

/// The verbs [`main`] runs: every trigger a runner takes, `add`, which also
/// registers a checkout, `issue`, which runs the issue writer itself, and
/// `attach` and `pm`, which run a worker's or the project manager's session
/// here, `usage`, which reads the usage ledger here, `watch`, which
/// follows the runner's log and the sessions' transcripts here, and `set` and
/// `get`, which change and read one setting of the project's table here
pub const VERBS: [&str; 23] = [
    "add", "start", "pause", "finish", "status", "rule", "rework", "adopt", "gate", "drop",
    "timings", "issue", "attach", "tell", "pm", "drain", "undrain", "usage", "watch", "board",
    "set", "get", "triage",
];

/// The verbs that print the runner's JSON instead of plain text with `--json`
const JSON_VERBS: [&str; 10] = [
    "add", "finish", "status", "gate", "drop", "timings", "tell", "drain", "undrain", "board",
];

/// What the verbs take, as their help says
pub const USAGE: &str = "\
usage: shep kelpie add [<project>]       registers this checkout as a project
       shep kelpie add <issue>           puts an issue on the project's board
       shep kelpie start | pause         runs the project, or stops it once its calls end
       shep kelpie finish                ends the open work items, picks nothing new, stops
       shep kelpie status [--json]       every project, or one with -p, as a short
                                         block each, or the runner's JSON object
       shep kelpie rule [<id> <answer>]
       shep kelpie rework <pr> | adopt <pr>
       shep kelpie gate [<issue>] | drop [<issue>]
       shep kelpie triage <issue>        triages an issue next, past the pick rule
       shep kelpie board                 the ready issues in the order the board takes them
       shep kelpie board move <issue> <position> | board top <issue>
                                         puts an issue at a place in that list, or first
       shep kelpie timings [<n>]         where the last n finished items' time went
       shep kelpie issue \"<request>\"     files issues for it, for you to read
       shep kelpie issue --interactive \"<request>\"
                                         plans them with you in claude
       shep kelpie attach <issue>        steers its worker's session here
       shep kelpie tell \"<note>\"         a note for the project manager's next wake
       shep kelpie pm                    steers the project manager's session here
       shep kelpie drain | undrain       holds back every new call, or lets them start
       shep kelpie get [<key>]           the project's settings, or one by its dotted key
       shep kelpie set <key> <value>     changes one setting, such as git.merging auto or
                                         worker.allowed_domains '[\"crates.io\"]'
       shep kelpie watch [<issue> | pm] [--back <n>] [--full]
                                         follows the runner's steps and the sessions'
                                         transcripts live, from the end or n lines back
       shep kelpie usage [<project>] [--since <date>]
                                         units and dollars per merged pull request
       shep kelpie usage <project> --import <steps file>
                                         adds a runner's logged calls to its ledger
       shep kelpie usage --forge [--since <date>]
                                         GitHub calls per minute, the busiest minutes and
                                         each used-up answer

`--json` goes anywhere in the line of `add <issue>`, `finish`, `status`, `gate`,
`drop`, `timings`, `tell`, `drain`, `undrain` and `board`, and prints the runner's
JSON object in place of plain text.

`-p <project>` or `--project <project>` goes anywhere in the line, before a
ruling's answer. Without it, the project is the one whose repo holds this
folder, or whose GitHub repo is this clone's `origin`.";

/// Runs `kelpie <command> <args>` for one of [`VERBS`]
pub fn main(command: &str, args: &[String]) -> ExitCode {
    let ran = crate::shep_home::required(crate::shep_home::FLOCK_FIX)
        .and_then(|shep_home| shepherd::block_on(run(&Socket, &shep_home, command, args)));
    match ran {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("shep kelpie {command}: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(
    dial: &dyn Dial,
    shep_home: &Path,
    command: &str,
    args: &[String],
) -> Result<Vec<String>, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is not set")?;
    let kelpie_home_set = std::env::var_os(crate::home::KELPIE_VAR)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let kelpie_home = crate::home::kelpie_home_of(shep_home);
    // A ruling's answer starts after its id and first word.
    let (named, args) = split_project(args, (command == "rule").then_some(2))?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // Only these verbs have a JSON form, so `--json` is a word to every other verb.
    let json = JSON_VERBS.contains(&command) && args.contains(&"--json");
    let args: Vec<&str> = if json {
        args.into_iter().filter(|arg| *arg != "--json").collect()
    } else {
        args
    };
    if command == "rule" {
        return answer(dial, shep_home, &home, &kelpie_home, named.as_ref(), &args).await;
    }
    if command == "usage" {
        return usage::usage(&kelpie_home, named, &args);
    }
    if command == "watch" {
        let ask = watch::read_args(&args)?;
        let here = std::env::current_dir().map_err(|e| format!("cannot read this folder: {e}"))?;
        // The shepherd is reached only to find the project from this folder.
        let project = match named {
            Some(name) => name,
            None => {
                let client = shepherd::connect(dial, shep_home)
                    .await
                    .map_err(|e| e.describe(shep_home))?;
                control::project_here(&client, &here, &home).await?
            }
        };
        watch::follow(&kelpie_home, &project, ask)?;
        return Ok(Vec::new());
    }
    let here = std::env::current_dir().map_err(|e| format!("cannot read this folder: {e}"))?;
    let client = shepherd::connect(dial, shep_home)
        .await
        .map_err(|e| e.describe(shep_home))?;
    // Named, or the one whose repo holds this folder.
    let project = async || match &named {
        Some(name) => Ok(name.clone()),
        None => control::project_here(&client, &here, &home).await,
    };
    let send = async |action: &str, params: Option<&str>| {
        control::send(&client, &project().await?, action, params).await
    };
    // The runner's JSON with `--json`, else its answer to `action` as plain lines.
    let print = async |action: &str, params: Option<&str>| {
        // The project is found before the action is sent, so a lookup that fails
        // cannot follow an action the runner already took.
        let name = project().await?;
        let answer = control::send(&client, &name, action, params).await?;
        Ok(match (json, answer.as_slice()) {
            (false, [body]) => control::plain_reply(name.as_str(), action, params, body),
            _ => answer,
        })
    };
    let format = if json { Format::Json } else { Format::Plain };
    // An action that changes the runner: one it takes late is no failure.
    let change = async |action: &str, params: Option<&str>| {
        let name = project().await?;
        control::change(&client, &name, action, params, format).await
    };
    // `start`, `pause`, `finish` and `add` take their project in place of `-p` too.
    let positional = |name: Option<&&str>| match (name, &named) {
        (Some(_), Some(_)) => Err(format!("name the project once\n\n{USAGE}")),
        (Some(name), None) => ProjectName::try_from(*name)
            .map(Some)
            .map_err(|e| e.to_string()),
        (None, named) => Ok(named.clone()),
    };
    match (command, args.as_slice()) {
        ("add", [issue]) if !issue.is_empty() && issue.bytes().all(|b| b.is_ascii_digit()) => {
            change("add", Some(issue)).await
        }
        // Registering a checkout has no JSON form.
        ("add", _) if json => Err(USAGE.to_owned()),
        ("add", [] | [_]) => {
            let checkout = Checkout::of(&here)?;
            let name = match positional(args.first())? {
                Some(name) => name,
                None => ProjectName::try_from(checkout.forge.name()).map_err(|e| e.to_string())?,
            };
            let launch = Launch {
                kelpie: std::env::current_exe()
                    .map_err(|e| format!("cannot find kelpie itself: {e}"))?,
                shep_home: shep_home.to_owned(),
                kelpie_home: kelpie_home_set,
            };
            let old_home = crate::home::old_home();
            let folder = kelpie_home.join(name.as_str());
            let agents = kelpie_home.join(crate::agents::FOLDER);
            let place = add::Place {
                checkout: &checkout,
                home: &home,
                folder: &folder,
                agents: &agents,
                old_home: old_home.as_deref(),
            };
            add::add(&client, &Gh::default(), &launch, &name, place).await
        }
        ("start" | "pause" | "finish", [] | [_]) => {
            let name = match positional(args.first())? {
                Some(name) => name,
                None => project().await?,
            };
            match command {
                "start" => control::start(&client, &name).await,
                "finish" => {
                    // What it waits on goes to stderr under `--json`, so stdout is the object.
                    let say = &mut |line: String| match json {
                        true => eprintln!("{line}"),
                        false => println!("{line}"),
                    };
                    control::finish(&client, &name, format, Patience::default(), say).await
                }
                _ => {
                    let say = &mut |line: String| println!("{line}");
                    let patience = Patience::default();
                    control::pause(&client, &name, patience, Interrupt::Signals, say).await
                }
            }
        }
        ("status", []) if named.is_none() => control::status(&client, format).await,
        ("status", []) => {
            let answer = send("status", None).await?;
            Ok(match (json, answer.as_slice()) {
                (false, [body]) => control::plain_status(project().await?.as_str(), body),
                _ => answer,
            })
        }
        ("rework" | "adopt" | "triage", [number]) => change(command, Some(number)).await,
        ("gate" | "drop", []) => change(command, None).await,
        ("gate" | "drop", [issue]) => change(command, Some(issue)).await,
        ("board", []) => print("board", None).await,
        ("board", ["top", issue]) if is_number(issue) => {
            change("board", Some(&format!("top {issue}"))).await
        }
        ("board", ["move", issue, position]) if is_number(issue) && is_number(position) => {
            change("board", Some(&format!("move {issue} {position}"))).await
        }
        ("timings", []) => print("timings", None).await,
        ("timings", [count]) => print("timings", Some(count)).await,
        ("drain" | "undrain", []) => change(command, None).await,
        ("attach", [issue]) => {
            let number = (issue.parse::<u64>().ok())
                .filter(|&n| n > 0 && issue.bytes().all(|b| b.is_ascii_digit()));
            let issue = number.ok_or_else(|| format!("{issue:?} is not an issue number"))?;
            attach::attach(&client, &project().await?, issue).await
        }
        ("tell", [_, ..]) => {
            let project = project().await?;
            control::tell(&client, &project, &args.join(" "), format).await
        }
        ("pm", []) => pm::pm(&client, &project().await?).await,
        ("get", [] | [_]) => set::get(&client, &project().await?, args.first().copied()).await,
        ("set", [key, value]) => {
            let name = project().await?;
            let paths = ProjectPaths::under(&kelpie_home, shep_home, &name);
            let place = set::Place {
                home: &home,
                paths: &paths,
            };
            set::set(&client, &name, place, key, value).await
        }
        ("issue", [_, ..]) => {
            let here = Here {
                client: &client,
                shep_home,
                kelpie_home: &kelpie_home,
                home: &home,
            };
            issue(here, &project().await?, &args).await
        }
        _ => Err(USAGE.to_owned()),
    }
}

// Whether `text` is digits only, as an issue or a position is.
fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

// Where `issue` finds the project's settings and files.
struct Here<'a> {
    client: &'a dyn Connection,
    shep_home: &'a Path,
    kelpie_home: &'a Path,
    home: &'a Path,
}

// `issue [--interactive] <request>`: the issue writer, run here rather than
// by the runner, since the maintainer waits on it or works in it.
async fn issue(here: Here<'_>, name: &ProjectName, args: &[&str]) -> Result<Vec<String>, String> {
    let (interactive, words) = match args {
        ["--interactive", words @ ..] => (true, words),
        words => (false, words),
    };
    let request = words.join(" ");
    if request.trim().is_empty() || request.starts_with('-') {
        return Err(USAGE.to_owned());
    }
    let tables = shepherd::read_tables_with(here.client, name.as_str()).await?;
    let paths = ProjectPaths::under(here.kelpie_home, here.shep_home, name);
    let loaded = source::load(&tables, name.as_str(), here.home, &paths.folder)
        .map_err(|e| e.to_string())?;
    crate::spawn::hide(loaded.kelpie.gateways().key_vars());
    let agents = Agents::load(&paths.agents).map_err(|e| e.to_string())?;
    let listed = (loaded.settings.role_agents(&agents)).map_err(|e| e.to_string())?;
    let writer = Writer::of(&agents, &loaded.settings.agents.issue_writer)?;
    let kelpie = std::env::current_exe().map_err(|e| format!("cannot find kelpie itself: {e}"))?;
    let remote = crate::runner::check_checkout(&loaded.settings).map_err(|e| e.to_string())?;
    let project = Project {
        settings: &loaded.settings,
        remote: &remote,
        paths: &paths,
        agents: &listed,
        kelpie: &kelpie,
    };
    if interactive {
        issues::make_labels(&Gh::default(), &project, Mode::Interactive)?;
        let mut claude = issues::interactive(&project, &writer, &request, OsStr::new("claude"))?;
        let status = claude.command.status();
        let landed = claude.end(&project, &Gh::default());
        let status = status.map_err(|e| format!("cannot run claude: {e}"))?;
        return match (landed, status.success()) {
            (landed, true) => landed,
            (Ok(mut lines), false) => {
                lines.insert(0, format!("claude ended with {status}"));
                Err(lines.join("\n"))
            }
            (Err(e), false) => Err(format!("claude ended with {status}\n{e}")),
        };
    }
    let tools = Tools::under(here.kelpie_home);
    if !tools.sandbox().is_file() {
        return Err(SandboxError::Missing(tools.sandbox()).to_string());
    }
    let codex_home =
        (loaded.kelpie.codex_home(here.home, here.kelpie_home)).map_err(|e| e.to_string())?;
    let secret_vars = loaded.kelpie.gateways().key_vars();
    let claude = ClaudeCli::default()
        .in_runtime(tools, here.home.to_owned(), &codex_home, secret_vars)
        .with_login(LoginToken::take());
    issues::headless(
        &project,
        &writer,
        &request,
        &Gh::default(),
        &claude,
        &SystemGit,
    )
}

// `rule`: the ruling and its answer read first, asking in a terminal, so
// the shepherd is reached to send it, and before that only to find the
// project of the clone a number is typed in.
async fn answer(
    dial: &dyn Dial,
    shep_home: &Path,
    home: &Path,
    kelpie_home: &Path,
    named: Option<&ProjectName>,
    args: &[&str],
) -> Result<Vec<String>, String> {
    let projects = Projects::under(kelpie_home);
    // Not in a clone, or no shepherd to ask, is a number with no project near.
    let here = match (named, args.first()) {
        (None, Some(first)) if rule::is_number(first) => {
            let here = std::env::current_dir().ok();
            match (shepherd::connect(dial, shep_home).await, here) {
                (Ok(client), Some(here)) => control::project_here(&client, &here, home).await.ok(),
                _ => None,
            }
        }
        _ => None,
    };
    let terminal = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let (mut input, mut output) = (std::io::stdin().lock(), std::io::stdout());
    let ask = terminal.then_some((
        &mut input as &mut dyn BufRead,
        &mut output as &mut dyn Write,
    ));
    let (project, params) = rule::prepare_in(&projects, named, here.as_ref(), args, ask)?;
    let client = shepherd::connect(dial, shep_home)
        .await
        .map_err(|e| e.describe(shep_home))?;
    let answers = ProjectPaths::under(kelpie_home, shep_home, &project).answers;
    control::rule(&client, &project, &params, &answers).await
}

/// `-p <project>`, `--project <project>` or `--project=<project>` from
/// anywhere in `args` before a `--`, and the rest in order
///
/// With `words_from`, everything after that many other arguments is words,
/// so a ruling's answer can carry `-p`.
///
/// # Errors
///
/// A message when the project is given twice, has no name after it, or is
/// not a project name.
pub fn split_project(
    args: &[String],
    words_from: Option<usize>,
) -> Result<(Option<ProjectName>, Vec<String>), String> {
    let mut named = None;
    let mut rest = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if words_from.is_some_and(|from| rest.len() >= from) {
            rest.push(arg.clone());
            continue;
        }
        let name = match arg.as_str() {
            "--" => {
                rest.extend(args.by_ref().cloned());
                break;
            }
            "-p" | "--project" => args
                .next()
                .ok_or_else(|| format!("{arg} takes a project's name"))?
                .as_str(),
            other => match other.strip_prefix("--project=") {
                Some(name) => name,
                None => {
                    rest.push(arg.clone());
                    continue;
                }
            },
        };
        if named.is_some() {
            return Err("name the project once".into());
        }
        named = Some(ProjectName::try_from(name).map_err(|e| e.to_string())?);
    }
    Ok((named, rest))
}

/// `args` with a verb that follows leading project flags moved first, so
/// `shep kelpie -p koji rule 14 yes` reaches `rule`
pub fn verb_first(mut args: Vec<String>) -> Vec<String> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        match arg.as_str() {
            "-p" | "--project" => at += 2,
            flag if flag.starts_with("--project=") => at += 1,
            verb if at > 0 && VERBS.contains(&verb) => {
                let verb = args.remove(at);
                args.insert(0, verb);
                break;
            }
            _ => break,
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn the_project_goes_anywhere_in_the_line_before_a_double_dash() {
        let koji = ProjectName::try_from("koji").ok();
        for line in [
            "-p koji 3 7",
            "3 7 -p koji",
            "3 --project koji 7",
            "3 7 --project=koji",
        ] {
            let (named, rest) = split_project(&words(line), None).unwrap();
            assert_eq!(named, koji, "{line}");
            assert_eq!(rest, words("3 7"), "{line}");
        }
        let (named, rest) = split_project(&words("14 no -- drop the -p flag"), None).unwrap();
        assert_eq!(named, None);
        assert_eq!(rest, words("14 no drop the -p flag"));
        for refused in ["-p", "-p koji -p rotom", "-p ../koji"] {
            assert!(split_project(&words(refused), None).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_ruling_s_answer_keeps_a_p_as_its_own_words() {
        let koji = ProjectName::try_from("koji").ok();
        for line in ["-p koji 14 yes", "14 -p koji yes", "14 --project=koji yes"] {
            let (named, rest) = split_project(&words(line), Some(2)).unwrap();
            assert_eq!(named, koji, "{line}");
            assert_eq!(rest, words("14 yes"), "{line}");
        }
        let line = words("14 no pass -p koji through");
        let (named, rest) = split_project(&line, Some(2)).unwrap();
        assert_eq!(named, None);
        assert_eq!(rest, line);
    }

    #[test]
    fn a_verb_after_the_project_goes_first() {
        let moved = |line| verb_first(words(line));
        assert_eq!(moved("-p koji rule 14 yes"), words("rule -p koji 14 yes"));
        assert_eq!(moved("--project=koji gate"), words("gate --project=koji"));
        assert_eq!(moved("-p koji timings 5"), words("timings -p koji 5"));
        assert_eq!(
            moved("-p koji issue --interactive add a thing"),
            words("issue -p koji --interactive add a thing")
        );
        assert_eq!(moved("-p koji attach 7"), words("attach -p koji 7"));
        assert_eq!(moved("-p koji tell hold #4"), words("tell -p koji hold #4"));
        assert_eq!(moved("rule -p koji 14 yes"), words("rule -p koji 14 yes"));
        assert_eq!(moved("-p koji runner x"), words("-p koji runner x"));
        assert_eq!(moved("-p"), words("-p"));
    }

    // `status` against a fake shepherd holding the runner `koji`.
    async fn status(args: &str) -> Result<Vec<String>, String> {
        verb("status", args, None).await
    }

    // `command args` against a fake shepherd holding the runner `koji`, which
    // answers every trigger with `says` when that is given.
    async fn verb(command: &str, args: &str, says: Option<&str>) -> Result<Vec<String>, String> {
        let shepherd = crate::test::FakeShepherd::new();
        let launch = Launch {
            kelpie: "/opt/kelpie".into(),
            shep_home: shepherd.home().to_owned(),
            kelpie_home: None,
        };
        let table = crate::test::project_table(include_str!("../../settings.example.toml"));
        let name = ProjectName::try_from("koji").unwrap();
        shepherd.holds(launch.runner(&name, table), true);
        shepherd.holds_dog("kelpie", true);
        if let Some(says) = says {
            shepherd.says("koji", &[says]);
        }
        // Only a bare `status` needs no project; the rest find theirs from the folder.
        let project = (command != "status").then_some("-p koji");
        let args: Vec<String> = (project.into_iter().flat_map(str::split_whitespace))
            .chain(args.split_whitespace())
            .map(str::to_owned)
            .collect();
        let dial = shepherd.dial();
        let ask = run(&*dial, shepherd.home(), command, &args);
        tokio::time::timeout(std::time::Duration::from_secs(40), ask)
            .await
            .unwrap_or_else(|_| panic!("{command} neither ended nor failed in time"))
    }

    // What `command args` prints plain, then with `--json`, for a runner that
    // answers `says`.
    async fn both(command: &str, args: &str, says: &str) -> (Vec<String>, Vec<String>) {
        let plain = verb(command, args, Some(says)).await.unwrap();
        let json = verb(command, &format!("{args} --json"), Some(says)).await;
        (plain, json.unwrap())
    }

    #[tokio::test]
    async fn timings_prints_the_table_plain_and_the_object_with_json() {
        let object = r#"{"seconds":{"worker":90},"table":"phase   seconds\nworker  90"}"#;
        let (plain, json) = both("timings", "20", object).await;
        assert_eq!(plain, ["phase   seconds", "worker  90"]);
        assert_eq!(json, [object]);
    }

    #[tokio::test]
    async fn board_prints_the_numbered_list_with_titles_and_priorities() {
        let list = r#"{"project":"koji","board":[
            {"position":1,"issue":284,"title":"Add the list","priority":"P0","blocked":false,"held":false},
            {"position":2,"issue":282,"title":"Order it","priority":null,"blocked":true,"held":false},
            {"position":3,"issue":285,"title":"Show it","priority":"P2","blocked":false,"held":true}]}"#;
        let (plain, json) = both("board", "", list).await;
        assert_eq!(
            plain,
            [
                "1. #284 Add the list (P0)",
                "2. #282 Order it (no priority, blocked)",
                "3. #285 Show it (P2, held)",
            ]
        );
        assert_eq!(json, [list]);
        let empty = r#"{"project":"koji","board":[]}"#;
        let (plain, _) = both("board", "", empty).await;
        assert_eq!(plain, ["koji: no issue is ready on the board"]);
    }

    #[tokio::test]
    async fn board_top_and_board_move_print_the_list_as_it_now_stands() {
        let list = r#"{"project":"koji","board":[
            {"position":1,"issue":282,"title":"Order it","priority":null,"blocked":false,"held":false},
            {"position":2,"issue":284,"title":"Add the list","priority":"P0","blocked":false,"held":false}]}"#;
        for args in ["top 282", "move 282 1"] {
            let (plain, _) = both("board", args, list).await;
            assert_eq!(
                plain,
                [
                    "1. #282 Order it (no priority)",
                    "2. #284 Add the list (P0)"
                ],
                "board {args}"
            );
        }
        for args in ["top", "move 282", "top x", "move 282 x", "up 282"] {
            let err = verb("board", args, Some(list)).await.unwrap_err();
            assert!(err.starts_with("usage: shep kelpie add"), "{args}: {err}");
        }
    }

    #[tokio::test]
    async fn drain_names_the_calls_still_running_and_undrain_says_calls_start() {
        let running =
            r#"{"draining":{"calls":[{"issue":7,"role":"worker"},{"role":"pm"}],"ceiling":3600}}"#;
        let (plain, json) = both("drain", "", running).await;
        assert_eq!(
            plain,
            ["koji is draining: no new call starts; still running: worker #7, pm"]
        );
        assert_eq!(json, [running]);
        let idle = r#"{"draining":{"calls":[],"ceiling":3600}}"#;
        let (plain, _) = both("drain", "", idle).await;
        assert_eq!(
            plain,
            ["koji is draining: no new call starts, and none is running"]
        );
        let status = r#"{"project":"koji","draining":null}"#;
        let (plain, json) = both("undrain", "", status).await;
        assert_eq!(plain, ["koji is not draining: calls start again"]);
        assert_eq!(json, [status]);
    }

    #[tokio::test]
    async fn drain_and_undrain_print_an_answer_that_contradicts_them_as_it_came() {
        let not_draining = r#"{"project":"koji","draining":null}"#;
        let (plain, _) = both("drain", "", not_draining).await;
        assert_eq!(plain, [not_draining]);
        let draining = r#"{"draining":{"calls":[],"ceiling":3600}}"#;
        let (plain, _) = both("undrain", "", draining).await;
        assert_eq!(plain, [draining]);
    }

    #[tokio::test]
    async fn gate_drop_and_add_say_what_they_did_to_the_work_item() {
        let status = r#"{"project":"koji","work_item":null}"#;
        for (command, args, line) in [
            (
                "gate",
                "",
                "koji: sent the open work item to the review gate",
            ),
            ("gate", "7", "koji: sent #7 to the review gate"),
            ("drop", "", "koji: dropped the open work item"),
            ("drop", "7", "koji: dropped #7"),
            ("add", "7", "koji: opened a work item for #7"),
        ] {
            let (plain, json) = both(command, args, status).await;
            assert_eq!(plain, [line], "{command} {args}");
            assert_eq!(json, [status], "{command} {args}");
        }
    }

    #[tokio::test]
    async fn tell_and_finish_print_their_line_plain_and_the_answer_with_json() {
        let told = r#"{"project":"koji"}"#;
        let (plain, json) = both("tell", "hold #4", told).await;
        assert_eq!(
            plain,
            ["told koji's project manager, which reads it on its next wake"]
        );
        assert_eq!(json, [told]);
        let finishing = r#"{"project":"koji","run":"finishing","work_items":[]}"#;
        let (plain, json) = both("finish", "", finishing).await;
        assert_eq!(
            plain,
            ["koji has no work item open, so its runner stops now"]
        );
        assert_eq!(json, [finishing]);
    }

    #[tokio::test]
    async fn a_refused_answer_prints_its_reason_and_fails_in_both_forms() {
        let refused = r#"{"error":"no work item for #8 is open"}"#;
        for (command, args) in [
            ("timings", "20"),
            ("board", ""),
            ("board", "top 8"),
            ("drain", ""),
            ("undrain", ""),
            ("gate", "8"),
            ("drop", "8"),
            ("tell", "hold #4"),
            ("add", "8"),
            ("finish", ""),
        ] {
            for flag in ["", " --json"] {
                let err = verb(command, &format!("{args}{flag}"), Some(refused)).await;
                assert_eq!(
                    err.unwrap_err(),
                    "no work item for #8 is open",
                    "{command} {args}{flag}"
                );
            }
        }
    }

    #[tokio::test]
    async fn json_is_refused_where_there_is_nothing_to_print_as_json() {
        for (command, args) in [("rework", "7 --json"), ("add", "--json"), ("pm", "--json")] {
            let err = verb(command, args, None).await.unwrap_err();
            assert!(
                err.starts_with("usage: shep kelpie add"),
                "{command}: {err}"
            );
        }
    }

    #[tokio::test]
    async fn status_prints_json_for_the_flag_wherever_it_stands() {
        let object = r#"{"action":"status","sheep":"koji"}"#;
        assert_eq!(status("--json").await.unwrap(), [format!("koji: {object}")]);
        for line in ["--json -p koji", "-p koji --json", "--project=koji --json"] {
            assert_eq!(status(line).await.unwrap(), [object], "{line}");
        }
    }

    #[tokio::test]
    async fn status_prints_a_plain_block_without_the_flag() {
        let block = [
            "koji",
            "  run: running",
            "  work items: none",
            "  rulings waiting: none",
            "  pacer: on",
        ];
        assert_eq!(status("").await.unwrap(), block);
        assert_eq!(status("-p koji").await.unwrap(), block);
    }

    #[tokio::test]
    async fn status_refuses_a_flag_it_does_not_know() {
        for line in ["--yaml", "--json --yaml", "-p koji --yaml", "--jsn"] {
            let err = status(line).await.unwrap_err();
            assert!(err.starts_with("usage: shep kelpie add"), "{line}: {err}");
        }
    }
}
