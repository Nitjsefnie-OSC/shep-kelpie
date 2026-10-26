//! The issue writer's `shep kelpie ticket` commands, on a project whose tickets
//! are the local store
//!
//! `ticket new` files under the rules `gh issue create` does, `ticket edit`
//! changes only labels and links of a ticket the session filed, and `ticket show`
//! and `list` read. Each names the session's project with `-p`: the verb
//! then works that project's folder without asking the shepherd, which the
//! writer's sandbox could not reach, and no other project's.

use super::gh::{Flags, Gh, check_filed, check_relabel, flags_of};
use super::{PLAIN, heredoc_body, only, plain};
use crate::ticket::TicketId;

const COMMAND: &str = "shep kelpie ticket";

const NOT_FILED: &str = "the issue writer edits and links only the tickets it filed in this \
    session";

/// What one `shep` command's words, after `shep`, may run on a local project
pub(super) fn judge(args: &[String], gh: &Gh<'_>) -> Result<(), String> {
    let [kelpie, ticket, verb, rest @ ..] = args else {
        return Err(only(gh.rules));
    };
    if kelpie != "kelpie" || ticket != "ticket" {
        return Err(only(gh.rules));
    }
    match verb.as_str() {
        "new" => new(rest, gh),
        "edit" => edit(rest, gh),
        "show" => show(rest, gh),
        "list" => list(rest, gh),
        _ => Err(only(gh.rules)),
    }
}

// The flag that names the project, which is this session's and no other.
const PROJECT: (&str, Option<&str>, bool) = ("--project", Some("-p"), true);

fn this_project(flags: &Flags, gh: &Gh<'_>) -> Result<(), String> {
    let project = gh.rules.tickets.as_deref().unwrap_or_default();
    let named: Vec<&str> = flags.all("--project").collect();
    match named.as_slice() {
        [name] if *name == project => Ok(()),
        _ => Err(format!(
            "name this project's tickets with `-p {project}`, once: the issue writer works no \
             other project's."
        )),
    }
}

// The flags' values besides `--body`, and the words, as plain text.
fn plain_but_body(flags: &Flags) -> Result<(), String> {
    let values = (flags.values.iter())
        .filter(|(name, _)| *name != "--body")
        .map(|(_, value)| value);
    match values.chain(&flags.words).all(|w| plain(w)) {
        true => Ok(()),
        false => Err(format!("{PLAIN}.")),
    }
}

// A local ticket's id, as the verb takes it.
fn local(word: &str) -> Result<TicketId, String> {
    match word.parse::<TicketId>() {
        Ok(ticket @ TicketId::Local(_)) => Ok(ticket),
        _ => Err(format!(
            "`{COMMAND}` takes the local tickets' ids, `k` and a number, and `{}` is not one.",
            word.chars()
                .take(crate::guard::SHOWN_CHARS)
                .collect::<String>()
        )),
    }
}

// The labels given, each whole: the verb splits none at a comma.
fn whole_labels<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    values
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

// The tickets a link flag names.
fn linked(flags: &Flags, flag: &str) -> Result<Vec<TicketId>, String> {
    flags.all(flag).map(local).collect()
}

// A parent is one of the tickets the session filed, as a sub-issue is.
fn parented(flags: &Flags, gh: &Gh<'_>) -> Result<(), String> {
    match linked(flags, "--parent")?
        .iter()
        .find(|p| !gh.ledger.filed(**p))
    {
        Some(parent) => Err(format!(
            "{NOT_FILED}: `--parent` must be one, and {parent} is not."
        )),
        None => Ok(()),
    }
}

fn new(args: &[String], gh: &Gh<'_>) -> Result<(), String> {
    let flags = flags_of(
        COMMAND,
        args,
        &[
            ("--body", None, true),
            ("--label", None, true),
            ("--parent", None, true),
            ("--blocked-by", None, true),
            PROJECT,
        ],
    )?;
    this_project(&flags, gh)?;
    plain_but_body(&flags)?;
    let [title] = flags.words.as_slice() else {
        return Err("give the ticket one title.".into());
    };
    let body = match flags.all("--body").collect::<Vec<_>>().as_slice() {
        [word] if heredoc_body(word) => gh.heredocs.join("\n"),
        [word] if plain(word) => (*word).to_owned(),
        [_] => return Err(format!("{PLAIN}.")),
        _ => {
            return Err("give the ticket one `--body`, as `--body \"$(cat <<'EOF' ...)\"`.".into());
        }
    };
    parented(&flags, gh)?;
    linked(&flags, "--blocked-by")?;
    let labels = whole_labels(flags.all("--label"));
    check_filed(title, &body, &labels, gh)
}

fn edit(args: &[String], gh: &Gh<'_>) -> Result<(), String> {
    let flags = flags_of(
        COMMAND,
        args,
        &[
            ("--label", None, true),
            ("--unlabel", None, true),
            ("--parent", None, true),
            ("--blocked-by", None, true),
            PROJECT,
        ],
    )?;
    this_project(&flags, gh)?;
    plain_but_body(&flags)?;
    let [word] = flags.words.as_slice() else {
        return Err(format!(
            "`{COMMAND} edit` takes the ticket's id, and only `--label`, `--unlabel`, \
             `--parent` or `--blocked-by`."
        ));
    };
    let ticket = local(word)?;
    let changes = ["--label", "--unlabel", "--parent", "--blocked-by"]
        .iter()
        .any(|flag| flags.all(flag).next().is_some());
    if !changes {
        return Err(format!(
            "`{COMMAND} edit` takes only `--label`, `--unlabel`, `--parent` or `--blocked-by`."
        ));
    }
    if !gh.ledger.filed(ticket) {
        return Err(format!("{NOT_FILED}, and {ticket} is not one."));
    }
    parented(&flags, gh)?;
    let mut links = linked(&flags, "--parent")?;
    links.extend(linked(&flags, "--blocked-by")?);
    if links.contains(&ticket) {
        return Err(format!(
            "{ticket} cannot be linked to itself: the board would never pick it."
        ));
    }
    let added = whole_labels(flags.all("--label"));
    let removed = whole_labels(flags.all("--unlabel"));
    check_relabel(&added, &removed, gh.rules)
}

fn show(args: &[String], gh: &Gh<'_>) -> Result<(), String> {
    let flags = flags_of(COMMAND, args, &[PROJECT])?;
    this_project(&flags, gh)?;
    plain_but_body(&flags)?;
    match flags.words.as_slice() {
        [word] => local(word).map(drop),
        _ => Err(format!(
            "`{COMMAND} show` takes one ticket of this project, by its id."
        )),
    }
}

fn list(args: &[String], gh: &Gh<'_>) -> Result<(), String> {
    let flags = flags_of(COMMAND, args, &[("--all", None, false), PROJECT])?;
    this_project(&flags, gh)?;
    plain_but_body(&flags)?;
    match flags.words.is_empty() {
        true => Ok(()),
        false => Err(format!(
            "`{COMMAND} list` takes only its flags, on this project."
        )),
    }
}
