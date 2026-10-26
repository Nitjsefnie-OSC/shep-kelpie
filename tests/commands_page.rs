//! The commands guide page names every `shep kelpie` verb and flag, and none
//! the command line lacks.
//!
//! The command line is the binary's usage text. The page is
//! `docs/guide/commands.md`. It names a verb with a code span or `sh` line that
//! starts with it (`board move <issue>` names `board move`), and a verb's flag
//! in one paragraph, item or line with a span for each. A flag every verb takes
//! (`-p`, `--json`) is named by any span. What the page names that the command
//! line lacks is checked in the spans it means as commands: `shep kelpie ...`,
//! a lowercase word then `<...>` or `[...]`, and a list entry beside a command.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

const KELPIE: &str = env!("CARGO_BIN_EXE_shep-kelpie");

/// Commands kelpie runs for itself, which no person types. Adding one here
/// stops the page being asked to name it.
const INTERNAL: [&str; 4] = ["runner", "guard", "confine", "mcp-connect"];

/// Named on the page though the command line has no verb of that name: a
/// trigger a person sends with `shep trigger`.
const TRIGGER_ONLY: [&str; 1] = ["handover"];

/// A word of a verb's name, or the name of a flag without its dashes
fn word_shaped(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_lowercase())
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The flag a word is, if it is one
fn flag_of(word: &str) -> Option<&str> {
    let word = word.trim_matches(|c| "[](),.;`".contains(c));
    let name = word.strip_prefix("--").or_else(|| word.strip_prefix('-'))?;
    word_shaped(name).then_some(word)
}

fn flags_in<'a>(words: impl IntoIterator<Item = &'a str>) -> impl Iterator<Item = String> {
    words.into_iter().filter_map(flag_of).map(str::to_owned)
}

/// The words after a leading `shep kelpie`, and whether it was there
fn after_program(words: &[&str]) -> (Vec<String>, bool) {
    match words {
        ["shep", "kelpie", rest @ ..] | ["shep-kelpie", rest @ ..] => {
            (rest.iter().map(|w| (*w).to_owned()).collect(), true)
        }
        _ => (words.iter().map(|w| (*w).to_owned()).collect(), false),
    }
}

/// `command` cut at the `|` outside brackets
fn alternatives(command: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (at, c) in command.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                parts.push(&command[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&command[start..]);
    parts
}

/// What the usage text defines
#[derive(Debug, Default)]
struct Cli {
    /// Each verb, by its words (`board move`), with the flags it takes
    verbs: BTreeMap<String, BTreeSet<String>>,
    /// Flags the usage text says go with any verb
    anywhere: BTreeSet<String>,
}

/// What a run of words on the page says about the verbs
#[derive(Debug, PartialEq, Eq)]
enum Reading {
    Verb(String),
    /// A verb that only has verbs under it (`lease`), named alone
    Group,
    Unknown(String),
}

impl Cli {
    fn read(usage: &str) -> Self {
        let mut cli = Self::default();
        for line in usage.lines() {
            let line = line.trim_start();
            let line = line.strip_prefix("usage: ").unwrap_or(line);
            let command = line
                .strip_prefix("shep-kelpie ")
                .or_else(|| line.strip_prefix("shep kelpie "));
            if let Some(command) = command {
                // A command is followed by two spaces and what it does.
                let command = command.split("  ").next().unwrap_or(command);
                let mut path = String::new();
                for alternative in alternatives(command) {
                    let words: Vec<&str> = alternative.split_whitespace().collect();
                    let own: Vec<&str> = words
                        .iter()
                        .copied()
                        .take_while(|w| word_shaped(w))
                        .collect();
                    if !own.is_empty() {
                        path = own.join(" ");
                    }
                    if !path.is_empty() {
                        cli.verbs
                            .entry(path.clone())
                            .or_default()
                            .extend(flags_in(words));
                    }
                }
            } else if line.starts_with("`-") {
                for span in line.split('`').skip(1).step_by(2) {
                    cli.anywhere
                        .extend(flags_in(span.split_whitespace().take(1)));
                }
            }
        }
        cli
    }

    fn typed(&self) -> impl Iterator<Item = (&String, &BTreeSet<String>)> {
        self.verbs.iter().filter(|(path, _)| {
            !path
                .split(' ')
                .next()
                .is_some_and(|w| INTERNAL.contains(&w))
        })
    }

    fn takes(&self, verb: &str, flag: &str) -> bool {
        self.anywhere.contains(flag) || self.verbs.get(verb).is_some_and(|f| f.contains(flag))
    }

    fn defines_flag(&self, flag: &str) -> bool {
        self.anywhere.contains(flag) || self.verbs.values().any(|flags| flags.contains(flag))
    }

    fn reading(&self, words: &[String]) -> Reading {
        let words: Vec<&str> = words
            .iter()
            .map(String::as_str)
            .take_while(|w| word_shaped(w))
            .collect();
        let group = |first: &str| {
            self.verbs
                .keys()
                .any(|path| path.split_once(' ').is_some_and(|(head, _)| head == first))
        };
        for len in (1..=words.len()).rev() {
            let path = words[..len].join(" ");
            // A verb that has verbs under it (`board`) is not read for a
            // second word it lacks (`board top`).
            if self.verbs.contains_key(&path) && !(len == 1 && words.len() > 1 && group(&path)) {
                return Reading::Verb(path);
            }
        }
        match words.as_slice() {
            [first, second, ..] if group(first) => Reading::Unknown(format!("{first} {second}")),
            [first] if group(first) => Reading::Group,
            [first, ..] => Reading::Unknown((*first).to_owned()),
            [] => Reading::Group,
        }
    }
}

/// One code span of the page, or one line of a `sh` block
#[derive(Debug)]
struct Span {
    /// The words that may start with a verb
    words: Vec<String>,
    /// Whether the page means this as a command, so what it names must exist
    command: bool,
    /// Whether it starts with another program, such as `shep trigger`
    foreign: bool,
    /// Every flag in the span, or in the whole line of a `sh` block
    flags: Vec<String>,
    /// Whether it starts with a flag, which then must exist
    opens_with_flag: bool,
    /// Whether only `,`, `and` or `or` separates it from the span before
    joined: bool,
}

impl Span {
    fn prose(code: &str, opens_item: bool, joined: bool) -> Self {
        let all: Vec<&str> = code.split_whitespace().collect();
        let (words, prefixed) = after_program(&all);
        let placeholder = words
            .iter()
            .skip(1)
            .any(|w| ["<", "[", "\"<"].iter().any(|open| w.starts_with(open)));
        let foreign = !prefixed && all.first() == Some(&"shep");
        let command = prefixed
            || (!foreign
                && words.first().is_some_and(|w| word_shaped(w))
                && (opens_item || placeholder));
        Self {
            words,
            command,
            foreign,
            flags: flags_in(all.iter().copied()).collect(),
            opens_with_flag: code.starts_with('-'),
            joined,
        }
    }

    fn line(line: &str) -> Self {
        let code = line.split('#').next().unwrap_or(line);
        let all: Vec<&str> = code.split_whitespace().collect();
        let (words, prefixed) = after_program(&all);
        Self {
            words,
            command: prefixed,
            foreign: false,
            flags: flags_in(line.split_whitespace()).collect(),
            opens_with_flag: false,
            joined: false,
        }
    }

    fn starts_with_word(&self) -> bool {
        !self.foreign && self.words.first().is_some_and(|w| word_shaped(w))
    }
}

/// Whether the text between two code spans only joins them in a list
fn joins(between: &str) -> bool {
    between
        .split_whitespace()
        .all(|w| matches!(w, "," | "and" | "or"))
}

/// The page's code, in the paragraphs, list items and `sh` block lines it
/// sits in
fn chunks(page: &str) -> Vec<Vec<Span>> {
    let mut chunks = Vec::new();
    let mut current: Vec<Span> = Vec::new();
    // Inside a fenced block: whether it is a `sh` one.
    let mut fence: Option<bool> = None;
    for line in page.lines() {
        let item = line.trim_start();
        if let Some(info) = item.strip_prefix("```") {
            chunks.push(std::mem::take(&mut current));
            fence = match fence {
                None => Some(info.trim() == "sh"),
                Some(_) => None,
            };
        } else if let Some(sh) = fence {
            if sh && !item.is_empty() {
                chunks.push(vec![Span::line(item)]);
            }
        } else {
            let bullet = item.strip_prefix("- ");
            if item.is_empty() || item.starts_with('#') || bullet.is_some() {
                chunks.push(std::mem::take(&mut current));
            }
            let body = bullet.unwrap_or(item);
            let pieces: Vec<&str> = body.split('`').collect();
            for at in (1..pieces.len()).step_by(2) {
                let opens_item = at == 1 && body.starts_with('`');
                current.push(Span::prose(
                    pieces[at],
                    opens_item,
                    at > 1 && joins(pieces[at - 1]),
                ));
            }
        }
    }
    chunks.push(current);
    chunks
}

/// Which of a chunk's spans are commands: those the page means as one, those
/// that name a verb, and those in a list with one (`finish`, `status` and `gate`)
fn commands(cli: &Cli, spans: &[Span]) -> Vec<bool> {
    let mut is: Vec<bool> = spans
        .iter()
        .map(|s| {
            s.command
                || (s.starts_with_word()
                    && !matches!(cli.reading(&s.words), Reading::Unknown(v) if !v.contains(' ')))
        })
        .collect();
    loop {
        let mut changed = false;
        for at in 0..spans.len() {
            let after = spans
                .get(at + 1)
                .is_some_and(|next| next.joined && is[at + 1]);
            let before = spans[at].joined && at > 0 && is[at - 1];
            if !is[at] && spans[at].starts_with_word() && (after || before) {
                is[at] = true;
                changed = true;
            }
        }
        if !changed {
            return is;
        }
    }
}

/// What is wrong with `page` against `usage`: one line for each verb or flag
/// that is missing from the page or stale on it
fn problems(usage: &str, page: &str) -> Vec<String> {
    let cli = Cli::read(usage);
    let chunks = chunks(page);
    // Per chunk, the verbs it names and the flags it has.
    let named: Vec<(BTreeSet<String>, BTreeSet<&String>)> = chunks
        .iter()
        .map(|spans| {
            let verbs = spans
                .iter()
                .filter(|s| s.words.first().is_some_and(|w| word_shaped(w)))
                .filter_map(|s| match cli.reading(&s.words) {
                    Reading::Verb(path) => Some(path),
                    _ => None,
                })
                .collect();
            (verbs, spans.iter().flat_map(|s| &s.flags).collect())
        })
        .collect();
    let mut missing_verbs = BTreeSet::new();
    let mut missing_flags = BTreeSet::new();
    for (path, flags) in cli.typed() {
        if !named.iter().any(|(verbs, _)| verbs.contains(path)) {
            missing_verbs.insert(format!("missing verb `{path}`"));
            continue;
        }
        for flag in flags {
            let together = named
                .iter()
                .any(|(verbs, has)| verbs.contains(path) && has.contains(flag));
            if !together {
                missing_flags.insert(format!("missing flag `{path} {flag}`"));
            }
        }
    }
    for flag in &cli.anywhere {
        if !named.iter().any(|(_, has)| has.contains(flag)) {
            missing_flags.insert(format!("missing flag `{flag}`"));
        }
    }
    let mut stale = BTreeSet::new();
    for (span, command) in chunks
        .iter()
        .flat_map(|spans| spans.iter().zip(commands(&cli, spans)))
    {
        if command
            && let Reading::Unknown(verb) = cli.reading(&span.words)
            && !TRIGGER_ONLY.contains(&verb.as_str())
        {
            stale.insert(format!("stale verb `{verb}`"));
        }
        if command || span.opens_with_flag {
            match cli.reading(&span.words) {
                // A verb takes its own flags and the ones every verb takes.
                Reading::Verb(path) if !span.opens_with_flag => {
                    for flag in span.flags.iter().filter(|f| !cli.takes(&path, f)) {
                        stale.insert(format!("stale flag `{path} {flag}`"));
                    }
                }
                _ => {
                    for flag in span.flags.iter().filter(|f| !cli.defines_flag(f)) {
                        stale.insert(format!("stale flag `{flag}`"));
                    }
                }
            }
        }
    }
    missing_verbs
        .into_iter()
        .chain(missing_flags)
        .chain(stale)
        .collect()
}

/// What the binary prints for arguments it does not take
fn usage() -> String {
    let output = Command::new(KELPIE)
        .arg("no-such-verb")
        .env_remove("SHEP_DOG_NAME")
        .env_remove("SHEP_NAME")
        .stdin(Stdio::null())
        .output()
        .expect("kelpie runs");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    String::from_utf8(output.stderr).expect("usage is text")
}

fn page() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/guide/commands.md"))
        .expect("the commands page is in the repo")
}

#[test]
fn every_verb_and_flag_the_cli_defines_is_on_the_commands_page_and_none_else() {
    let problems = problems(&usage(), &page());
    assert!(
        problems.is_empty(),
        "docs/guide/commands.md and the command line disagree:\n  {}\n\
         Name each missing verb or flag in backticks on the page, drop each stale one, \
         or for a command no person types, add it to INTERNAL in tests/commands_page.rs.",
        problems.join("\n  ")
    );
}

#[test]
fn the_usage_text_lists_every_verb_the_flock_runs() {
    let cli = Cli::read(&usage());
    let missing: Vec<&str> = shep_kelpie::flock::VERBS
        .into_iter()
        .filter(|verb| !cli.verbs.contains_key(*verb))
        .collect();
    assert!(missing.is_empty(), "usage lacks {missing:?}");
}

#[test]
fn a_flag_deleted_from_the_commands_page_is_named_with_its_verb() {
    let without = page().replace("--rollback", "");
    let problems = problems(&usage(), &without);
    assert!(
        problems.contains(&"missing flag `upgrade --rollback`".to_owned()),
        "{problems:?}"
    );
}

#[test]
fn the_usage_text_reads_into_nested_verbs_and_their_flags() {
    let cli = Cli::read(&usage());
    for (verb, flag) in [
        ("board move", None),
        ("board top", None),
        ("lease run", None),
        ("github setup", Some("--replace")),
        ("upgrade", Some("--rollback")),
        ("totp", Some("--unlock")),
        ("watch", Some("--full")),
        ("usage", Some("--forge")),
    ] {
        let flags = cli.verbs.get(verb).unwrap_or_else(|| panic!("no `{verb}`"));
        if let Some(flag) = flag {
            assert!(flags.contains(flag), "`{verb}` lacks {flag}: {flags:?}");
        }
    }
    assert!(cli.anywhere.contains("-p") && cli.anywhere.contains("--json"));
}

mod small_cli {
    use super::*;

    const USAGE: &str = "\
usage: shep-kelpie add [<project>] | add <issue>
       shep-kelpie board move <issue> <position> | board top <issue>
       shep-kelpie upgrade [--now] --ref <git ref> | --rollback
       shep-kelpie lease run <kind> -- <command> [args...]
       shep-kelpie runner <project>
       shep-kelpie guard <git common dir> <worktree>
       shep-kelpie confine <folder>...

`--json` goes anywhere in the line of `add <issue>`.";

    const PAGE: &str = "\
# Commands

```sh
shep kelpie add        # a project
shep kelpie upgrade --ref main   # or --rollback, or --now
```

Every trigger is a verb: `add <issue>`, `board move <issue> <position>`, `board top <issue>`. `--json` prints JSON.

`lease run` waits for a lease.
";

    fn without(text: &str) -> String {
        PAGE.replace(text, "")
    }

    #[test]
    fn a_page_naming_everything_the_cli_defines_passes_with_internal_commands_left_out() {
        assert_eq!(problems(USAGE, PAGE), Vec::<String>::new());
    }

    #[test]
    fn a_nested_verb_missing_from_the_page_is_named() {
        let page = without("`board move <issue> <position>`, ");
        assert_eq!(problems(USAGE, &page), ["missing verb `board move`"]);
    }

    #[test]
    fn a_verb_on_the_page_only_inside_a_longer_one_is_missing() {
        let usage = format!("{USAGE}\n       shep-kelpie board");
        assert_eq!(problems(&usage, PAGE), ["missing verb `board`"]);
    }

    #[test]
    fn a_flag_missing_from_the_page_is_named_with_its_verb() {
        let page = without("or --rollback, ");
        assert_eq!(
            problems(USAGE, &page),
            ["missing flag `upgrade --rollback`"]
        );
    }

    #[test]
    fn a_flag_named_only_beside_another_verb_is_missing_for_its_own_verb_and_stale_there() {
        let page =
            without("or --rollback, ").replace("`lease run` waits", "`add --rollback` waits");
        assert_eq!(
            problems(USAGE, &page),
            [
                "missing verb `lease run`",
                "missing flag `upgrade --rollback`",
                "stale flag `add --rollback`"
            ]
        );
    }

    #[test]
    fn a_flag_every_verb_takes_missing_from_the_page_is_named() {
        let page = without(" `--json` prints JSON.");
        assert_eq!(problems(USAGE, &page), ["missing flag `--json`"]);
    }

    #[test]
    fn a_verb_the_cli_no_longer_defines_is_stale() {
        let page =
            format!("{PAGE}\n`wander <issue>` sends it off, and `shep kelpie lease fetch`.\n");
        assert_eq!(
            problems(USAGE, &page),
            ["stale verb `lease fetch`", "stale verb `wander`"]
        );
    }

    #[test]
    fn a_flag_the_cli_no_longer_defines_is_stale() {
        let page = format!("{PAGE}\nPass `--hurry` to `add <issue>`.\n");
        assert_eq!(problems(USAGE, &page), ["stale flag `--hurry`"]);
    }

    #[test]
    fn code_that_is_no_command_is_left_alone() {
        let page = format!(
            "{PAGE}\nThe state names `work_items` and `run: finishing`, and `jq -r .pacer` reads it.\n\
             ```text\nshep kelpie vanish --gone\n```\n"
        );
        assert_eq!(problems(USAGE, &page), Vec::<String>::new());
    }

    #[test]
    fn a_flag_the_cli_lacks_is_stale_in_any_span_that_names_a_verb() {
        let page = format!("{PAGE}\nSee `add <issue>` and `upgrade --bogus`.\n");
        assert_eq!(problems(USAGE, &page), ["stale flag `upgrade --bogus`"]);
    }

    #[test]
    fn a_bare_verb_in_a_list_with_commands_is_stale_when_the_cli_lacks_it() {
        let page = format!("{PAGE}\nAlso `add <issue>`, `drain` and `finish`.\n");
        assert_eq!(
            problems(USAGE, &page),
            ["stale verb `drain`", "stale verb `finish`"]
        );
    }

    #[test]
    fn a_nested_verb_the_cli_lacks_is_stale_without_a_placeholder() {
        let page = format!("{PAGE}\n`lease take` holds one.\n");
        assert_eq!(problems(USAGE, &page), ["stale verb `lease take`"]);
    }

    #[test]
    fn a_flag_that_belongs_to_another_verb_is_stale_for_this_one() {
        let page = format!("{PAGE}\n`add --rollback` is not a thing.\n");
        assert_eq!(problems(USAGE, &page), ["stale flag `add --rollback`"]);
    }

    #[test]
    fn a_nested_verb_the_cli_lacks_is_stale_under_a_verb_that_stands_alone() {
        let usage = USAGE.replace(" | board top <issue>", "") + "\n       shep-kelpie board";
        let page = format!("{PAGE}`board`, `board move <issue> <position>`\n");
        assert_eq!(problems(&usage, &page), ["stale verb `board top`"]);
    }

    #[test]
    fn plain_words_in_a_list_are_left_alone() {
        let page = format!("{PAGE}\nIt ends `ok`, `failed` or `held`, for `login` or `app`.\n");
        assert_eq!(problems(USAGE, &page), Vec::<String>::new());
    }

    #[test]
    fn a_trigger_with_no_verb_may_be_named() {
        let page = format!("{PAGE}\n- `handover` replaces the image\n");
        assert_eq!(problems(USAGE, &page), Vec::<String>::new());
    }
}
