//! `gh` in a worker's Bash call
//!
//! A pull request's title must be a conventional commit, and no title, body
//! or notes may carry what [`LocalPaths`](crate::local_paths::LocalPaths)
//! finds of this machine's. What only the project manager does is refused
//! whatever the command's shape: merging, marking ready, summoning a review,
//! and `gh api` and `gh auth`, which reach around them. Outside a quoted
//! `--body`, `--title` or `--notes` value a word holds only letters, digits
//! and `. _ / : @ = , + -`, since a shell could build any other. Only gh's own
//! commands run, as an alias or extension could stand for any of those.

use std::path::Path;

use super::shell::Quoting;
use super::{Home, SHOWN_CHARS, WRITE, files, values};
use crate::coderabbit::LABEL;
use crate::hooks::Role;

/// The conventional commit types a pull request title may start with
pub const TYPES: [&str; 11] = [
    "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert",
];

// gh's own commands and their short names, besides `api`, `auth`, `alias`,
// `co` and `extension`, which are refused by name.
const COMMANDS: [&str; 31] = [
    "agent-task",
    "attestation",
    "browse",
    "cache",
    "codespace",
    "completion",
    "config",
    "copilot",
    "cs",
    "discussion",
    "gist",
    "gpg-key",
    "help",
    "issue",
    "label",
    "licenses",
    "org",
    "pr",
    "preview",
    "project",
    "release",
    "repo",
    "ruleset",
    "run",
    "search",
    "secret",
    "skill",
    "ssh-key",
    "status",
    "variable",
    "workflow",
];

const MANAGER_ONLY: &str = "only the project manager merges a pull request, marks one ready \
                            or summons a review, on the maintainer's ruling: leave your pull \
                            request as a draft.";

// A worker's `gh` is the maintainer's login, and its comment on its own pull
// request can read as the maintainer's answer to a ruling. `close`, `reopen`,
// `review` and `merge` also take a comment or body, so any verb but `create`
// and `edit` that carries one is refused.
const NO_COMMENTS: &str = "a worker posts no comment or review text, nor closes or reopens \
                           with one: kelpie posts what the maintainer should read, and the \
                           maintainer's login is the one that answers a ruling. Put what you \
                           have to say in your final message.";

const FLAG_FIRST: &str = "kelpie reads a gh command's group and verb by position, so only \
                          `-R` or `--repo` with its value may come before them: gh takes any \
                          other flag there with the next word as its value. Put the flag \
                          after the verb.";

// A character any gh word may be made of. A shell leaves these as they are, so
// the guard reads what gh gets. A list of the shell's expansions would miss
// the next one: extended globs and zsh's. A word may not start with `=`, which
// zsh expands.
fn plain(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | ':' | '@' | '=' | ',' | '+' | '-')
}

// Where a `--body`, `--title` or `--notes` value starts in `word`, if it carries one, and
// sets `next` when the value is the next word.
fn text_from(word: &str, next: &mut bool) -> Option<usize> {
    let flag = word.split_once('=').map_or(word, |(flag, _)| flag);
    if matches!(flag, "--body" | "--title" | "--notes") {
        // `--body=text` carries its value, `--body text` is the next word.
        *next = !word.contains('=');
        return Some(if *next { word.len() } else { flag.len() + 1 });
    }
    // `-btext` and `-ttext`, one word.
    if word.starts_with("-b") || word.starts_with("-t") {
        *next = word.len() == 2;
        return Some(2);
    }
    None
}

// How many characters of `word` a shell reads as a redirection: `>`, `2>`, `<<`.
// It builds no word for gh, and a heredoc's `<<` is how `--body-file -` is fed.
// The `&` of `2>&1`, `<&0` and `&>out` is part of it: the shell splitter keeps it in the word.
fn redirect_len(word: &str, quoted: impl Fn(usize) -> bool) -> usize {
    let digits = word.chars().take_while(char::is_ascii_digit).count();
    let lead = if word.starts_with('&') { 1 } else { digits };
    let marks = word
        .chars()
        .skip(lead)
        .take_while(|c| matches!(c, '<' | '>'))
        .count();
    let dup = usize::from(word.chars().nth(lead + marks) == Some('&'));
    match marks {
        0 => 0,
        _ if (0..lead + marks + dup).any(quoted) => 0,
        _ => lead + marks + dup,
    }
}

// The first word of a gh command that a shell may build or expand: one with a
// character outside [`plain`], besides the characters a quote holds in a
// `--body`, `--title` or `--notes` value, which is text. A quoted word is
// checked like a bare one elsewhere; a word `quoting` lacks is bare.
fn built_word<'a>(words: &'a [String], quoting: &[Quoting]) -> Option<&'a str> {
    let mut value = false;
    for (at, word) in words.iter().enumerate().skip(1) {
        let quoting = quoting.get(at);
        let held = |i: usize| quoting.is_some_and(|q| q.quoted.get(i) == Some(&true));
        // An unquoted `(` after a word may make it one glob with the next.
        if quoting.is_some_and(|q| q.glued) {
            return Some(word);
        }
        let from = if std::mem::take(&mut value) {
            Some(0)
        } else {
            text_from(word, &mut value)
        };
        let skip = match from {
            None => redirect_len(word, held),
            Some(_) => 0,
        };
        let bad = word.chars().enumerate().skip(skip).any(|(i, c)| {
            let text = from.is_some_and(|from| i >= from) && held(i);
            // zsh turns a bare `=name` at the start of a word into a path.
            !text && (!plain(c) || (i == 0 && c == '='))
        });
        if bad {
            return Some(word);
        }
    }
    None
}

// The groups refused by name, whatever flags and verb follow.
fn refused_by_name(group: &str) -> bool {
    matches!(
        group,
        "api" | "auth" | "alias" | "co" | "extension" | "extensions" | "ext"
    )
}

// The verbs that only read, whose `-c` is `--comments`.
const READS: [&str; 5] = ["view", "list", "status", "diff", "checks"];

// Whether a command's arguments carry a comment or a body, in any spelling:
// `--comment`, `--comment=x`, `-c`, `-cx`, `-xc`, and the same for `--body`
// (`-b`) and `--body-file` (`-F`). The flag counts however its value reads.
fn carries_text(args: &[String]) -> bool {
    const LONG: [&str; 3] = ["--comment", "--body", "--body-file"];
    const SHORT: [char; 3] = ['c', 'b', 'F'];
    (args.iter().take_while(|a| *a != "--")).any(|word| {
        if word.starts_with("--") {
            let name = word.split_once('=').map_or(word.as_str(), |(name, _)| name);
            return LONG.contains(&name);
        }
        (word.strip_prefix('-')).is_some_and(|cluster| cluster.contains(SHORT))
    })
}

/// Judges one gh command run in `cwd`, `quoting` saying how each of `words` was written
pub(super) fn judge(
    words: &[String],
    quoting: &[Quoting],
    heredocs: &[String],
    cwd: Option<&Path>,
    home: &Home,
) -> Vec<String> {
    let mut found = read(words, heredocs, cwd, home);
    // Each word is what gh gets, so no word may be one a shell builds. The rest
    // is judged as the words read, so it may also say what else is wrong.
    if let Some(word) = built_word(words, quoting) {
        found.insert(
            0,
            format!(
                "kelpie cannot check a gh command whose words a shell builds or expands, \
                 and `{}` holds a character outside letters, digits and `. _ / : @ = , + -`: \
                 write each word out with those. Only a quoted `--body`, `--title` or \
                 `--notes` value may hold others.",
                word.chars().take(SHOWN_CHARS).collect::<String>()
            ),
        );
    }
    found
}

// What is wrong with a gh command, reading its words as gh gets them.
fn read(words: &[String], heredocs: &[String], cwd: Option<&Path>, home: &Home) -> Vec<String> {
    // The group and verb are the first two words that are not flags, and the
    // only flag that may come before them is `-R` or `--repo` with its value:
    // `gh pr -R owner/repo create`. gh takes any other flag there with the next
    // word as its value, so a verb the guard found could be another to gh.
    let mut positions: Vec<usize> = Vec::new();
    let mut i = 1;
    while i < words.len() && positions.len() < 2 {
        match words[i].as_str() {
            "-R" | "--repo" => i += 1,
            w if w.starts_with("--repo=") || (w.starts_with("-R") && !w.starts_with("--")) => {}
            // Refused by name below whatever follows, with their own reasons.
            w if w.starts_with('-')
                && positions
                    .first()
                    .is_some_and(|&g| refused_by_name(words[g].as_str())) => {}
            // Nothing follows to misread.
            "--version" | "--help" | "-h" if words.len() == 2 => {}
            w if w.starts_with('-') => return vec![FLAG_FIRST.into()],
            _ => positions.push(i),
        }
        i += 1;
    }
    let Some(&group) = positions.first() else {
        return Vec::new();
    };
    let group = words[group].as_str();
    let verb = positions.get(1).map(|&v| match words[v].as_str() {
        "new" => "create",
        verb => verb,
    });
    let args = &words[i.min(words.len())..];
    match (group, verb) {
        ("api", _) => {
            return vec![
                "kelpie runs no `gh api` for a worker: it reaches around what only the \
                 project manager does. Use gh's own commands."
                    .into(),
            ];
        }
        ("auth", _) => {
            return vec!["`gh auth` handles the maintainer's token, which no worker needs.".into()];
        }
        ("pr", Some("merge" | "ready")) => return vec![MANAGER_ONLY.into()],
        ("pr" | "issue", Some("comment")) => return vec![NO_COMMENTS.into()],
        // An allow-list: only create and edit may carry text, so the next verb
        // gh adds is covered too. What only reads never posts.
        ("pr" | "issue" | "discussion", Some(verb))
            if !matches!(verb, "create" | "edit")
                && !READS.contains(&verb)
                && carries_text(args) =>
        {
            return vec![NO_COMMENTS.into()];
        }
        ("alias" | "co" | "extension" | "extensions" | "ext", _) => {
            return vec![
                "kelpie runs no gh alias or extension: run the gh command it stands for.".into(),
            ];
        }
        (group, _) if !COMMANDS.contains(&group) => {
            return vec![format!(
                "kelpie runs only gh's own commands, and `{}` is not one: run the command it \
                 stands for.",
                group.chars().take(SHOWN_CHARS).collect::<String>()
            )];
        }
        _ => {}
    }
    // Labels come comma-separated, and GitHub matches their names in any case.
    let summons = values(args, &["--add-label", "--label"], &['l'])
        .iter()
        .flat_map(|v| v.split(','))
        .any(|label| {
            label
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .eq_ignore_ascii_case(LABEL)
        });
    if summons {
        return vec![MANAGER_ONLY.into()];
    }
    let Some(verb) = verb else {
        return Vec::new();
    };
    let publishes = matches!(
        (group, verb),
        ("pr", "create" | "edit" | "review")
            | ("issue", "create" | "edit")
            | ("release", "create" | "edit")
    );
    if !publishes {
        return Vec::new();
    }
    let titles = values(args, &["--title"], &['t']);
    let mut out = Vec::new();
    if group == "pr" && verb == "create" && titles.is_empty() {
        out.push(
            "`gh pr create` needs `--title`: without one, GitHub titles a pull request of \
             several commits with the branch's name. Give it a conventional commit subject, \
             such as `fix(parser): keep the last line`."
                .to_owned(),
        );
    }
    // Not echoed: a title can carry a path too.
    if group == "pr" && !titles.iter().all(|t| conventional_type(t).is_some()) {
        out.push(format!(
            "this pull request's title is not a conventional commit. Write it as \
             `type(scope): summary`, the scope optional, with the type one of {}, and \
             `!` after the type or scope for a breaking change.",
            TYPES.join(", ")
        ));
    }
    let bodies: Vec<String> = values(args, &["--body", "--notes"], &['b', 'n'])
        .into_iter()
        .chain(files(args, &["--body-file", "--notes-file"], &['F'], cwd))
        .chain(heredocs.iter().cloned())
        .collect();
    let texts = titles.iter().chain(&bodies).cloned();
    if let Some(leak) = home.find_in_prose(texts) {
        out.push(home.refusal(&format!("this `gh {group} {verb}`"), leak, WRITE));
        return out;
    }
    // The project's own hooks judge what the guard let through.
    let fields = [("title", &titles), ("body", &bodies)];
    for (field, texts) in fields {
        let field = format!("the `gh {group} {verb}` {field}");
        out.extend(
            texts
                .iter()
                .find_map(|t| home.publish(Role::Worker, &field, t)),
        );
    }
    out
}

/// The type `title` starts with as a conventional commit subject, as `fix`
/// of `fix(board): a thing`, or none when it is not one
pub fn conventional_type(title: &str) -> Option<&'static str> {
    let (head, summary) = title.split_once(": ")?;
    let head = head.strip_suffix('!').unwrap_or(head);
    let kind = match head.split_once('(') {
        Some((kind, scope)) => {
            let scope = scope.strip_suffix(')')?;
            if scope.is_empty() || scope.contains(['(', ')', '\n']) {
                return None;
            }
            kind
        }
        None => head,
    };
    let kind = TYPES.iter().copied().find(|&t| t == kind)?;
    (!summary.trim().is_empty()).then_some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every command `gh help` lists in gh 2.96.0, `co` its one alias.
    const GH_HELP: [&str; 35] = [
        "auth",
        "browse",
        "codespace",
        "discussion",
        "gist",
        "issue",
        "org",
        "pr",
        "project",
        "release",
        "repo",
        "skill",
        "cache",
        "run",
        "workflow",
        "co",
        "agent-task",
        "alias",
        "api",
        "attestation",
        "completion",
        "config",
        "copilot",
        "extension",
        "gpg-key",
        "label",
        "licenses",
        "preview",
        "ruleset",
        "search",
        "secret",
        "ssh-key",
        "status",
        "variable",
        "help",
    ];

    // A hook that was given no folders and no names.
    fn nobody() -> Home {
        Home::new(None, crate::local_paths::LocalPaths::default())
    }

    #[test]
    fn every_command_gh_lists_is_known_by_name() {
        for name in GH_HELP {
            let found = judge(
                &["gh", name, "list"].map(str::to_owned),
                &[],
                &[],
                None,
                &nobody(),
            );
            assert!(
                found.iter().all(|f| !f.contains("not one")),
                "{name}: {found:?}"
            );
        }
        let found = judge(
            &["gh", "workflow", "view", "ci"].map(str::to_owned),
            &[],
            &[],
            None,
            &nobody(),
        );
        assert!(found.is_empty(), "{found:?}");
    }
}
