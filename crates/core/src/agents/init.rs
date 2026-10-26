//! The models each role runs on each harness, and the lineup `agents init` writes
//!
//! `agents/models.toml` is kelpie's own, read from its build and never written
//! to the operator's folder: a harness, then a role, then the `model` and
//! `effort` it runs, with `second_look` and `context` where they apply, and
//! `order`, the harnesses `--detect` prefers. A role with several baselines
//! names each one, and its file is named for it. [`read`] parses it and refuses
//! a name that is no harness, no role, or no baseline of that role.
//! [`File::bind`] gives each role the first harness in `order` that was found
//! and serves it. [`write`] writes a bound file for each, never over one.

use std::fmt;
use std::fs;
use std::path::Path;

use super::{Role, baseline, extends, write_new};
use crate::settings::{ContextSize, Effort, GatewayName, Harness, ModelHost};

/// kelpie's own models file, read from its build
pub const MODELS: &str = include_str!("../../../../agents/models.toml");

/// The data file, as a refusal names it
pub const FILE: &str = "agents/models.toml";

/// The keys a row holds beside its harness and role
const KEYS: [&str; 4] = ["model", "effort", "second_look", "context"];

/// The levels `effort` takes
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// One row: one role on one harness, at a model and an effort
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The harness it runs the role on
    pub harness: Harness,
    /// What the file is for
    pub role: Role,
    /// The baseline its file extends, none for an implementer
    pub baseline: Option<&'static str>,
    /// The model it runs
    pub model: String,
    /// The effort it runs it at
    pub effort: Effort,
    /// Whether the reviewer reads twice
    pub second_look: bool,
    /// The tokens its model gives, for a harness that runs it on a server
    pub context: Option<u32>,
}

impl Row {
    /// The file it writes: its baseline, or the role's name where the role has none
    pub fn file(&self) -> &'static str {
        self.baseline.unwrap_or_else(|| self.role.as_str())
    }

    /// Whether its harness runs its model on a server, so `init` needs one named
    pub fn needs_server(&self) -> bool {
        self.harness.facts().needs_model_server
    }

    /// The file's text, with `server` where its harness runs its model on one
    pub fn text(&self, server: Option<&ModelHost>) -> String {
        let mut text = match self.baseline {
            Some(baseline) => format!("---\nextends: {}{baseline}\n", extends::SHIPPED),
            None => format!("---\nrole: {}\n", self.role.as_str()),
        };
        text.push_str(&format!(
            "harness: {}\nmodel: {}\neffort: {}\n",
            self.harness.as_str(),
            quoted(&self.model),
            self.effort.as_str()
        ));
        if self.second_look {
            text.push_str("second_look: true\n");
        }
        match server {
            Some(ModelHost::Gateway(gateway)) => {
                text.push_str(&format!("gateway: {}\n", quoted(gateway.as_str())));
            }
            Some(ModelHost::Url(url)) => text.push_str(&format!("url: {}\n", quoted(url.as_str()))),
            None => {}
        }
        if let Some(tokens) = self.context {
            text.push_str(&format!("context: {tokens}\n"));
        }
        text.push_str("---\n");
        text
    }
}

/// `value` as a YAML string: as it stands when that reads back the same, else in quotes
pub fn quoted(value: &str) -> String {
    let plain = value.starts_with(|c: char| c.is_ascii_alphabetic())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/:".contains(c))
        && !matches!(value, "true" | "false" | "null" | "yes" | "no");
    match plain {
        true => value.to_owned(),
        false => serde_json::to_string(value).unwrap_or_default(),
    }
}

/// What the models file holds
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// The harnesses `--detect` prefers, first to last
    pub order: Vec<Harness>,
    /// Every row, in the order the file holds them
    pub rows: Vec<Row>,
}

impl File {
    /// The rows for `harness`, in the order the file holds them
    pub fn rows_for(&self, harness: Harness) -> Vec<&Row> {
        self.rows
            .iter()
            .filter(|row| row.harness == harness)
            .collect()
    }

    /// The row for `role`'s `baseline` on `harness`
    pub(crate) fn row(&self, harness: Harness, role: Role, baseline: Option<&str>) -> Option<&Row> {
        let named = |given: Option<&str>| given.map(str::to_owned);
        self.rows.iter().find(|row| {
            row.harness == harness && row.role == role && named(row.baseline) == named(baseline)
        })
    }

    /// What `--detect` binds from what it found: each role on the first harness
    /// in `order` that was found, serves it, and has a row whose model a gateway
    /// in kelpie's settings lists, and a line for each role that got none
    pub fn bind(&self, found: &Found) -> Bindings {
        let mut bindings = Bindings::default();
        for (role, baseline) in slots() {
            let mut why: Vec<String> = Vec::new();
            let mut got: Option<Choice> = None;
            for &harness in &self.order {
                if !found.harnesses.contains(&harness) {
                    why.push(format!("{} is not on the PATH", harness.command()));
                    continue;
                }
                if !harness.serves(role) {
                    why.push(lacks(role, harness));
                    continue;
                }
                let Some(row) = self.row(harness, role, baseline) else {
                    why.push(format!("{} has no row in {FILE}", harness.as_str()));
                    continue;
                };
                let server = match row.needs_server() {
                    false => None,
                    true => match found.gateway_for(&row.model) {
                        Some(gateway) => Some(ModelHost::Gateway(gateway.clone())),
                        None => {
                            why.push(format!(
                                "{} runs {}, which no gateway in kelpie's settings lists",
                                harness.as_str(),
                                row.model
                            ));
                            continue;
                        }
                    },
                };
                got = Some(Choice {
                    row: row.clone(),
                    server,
                });
                break;
            }
            match got {
                Some(choice) => bindings.choices.push(choice),
                None => bindings.none.push(format!(
                    "{}: none: {}",
                    baseline.unwrap_or(role.as_str()),
                    why.join("; ")
                )),
            }
        }
        bindings
    }
}

/// A gateway `--detect` asked for the models it serves
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// Its name in kelpie's settings
    pub name: GatewayName,
    /// The models it listed
    pub models: Vec<String>,
    /// Why it answered with none
    pub why: Option<String>,
}

/// What `--detect` found on this machine
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// The harnesses whose program is on the `PATH`
    pub harnesses: Vec<Harness>,
    /// Each gateway kelpie's settings hold, with what it answered
    pub gateways: Vec<Listed>,
}

impl Found {
    /// The gateway that lists `model`, if one of them does
    pub fn gateway_for(&self, model: &str) -> Option<&GatewayName> {
        self.gateways
            .iter()
            .find(|listed| listed.models.iter().any(|m| m == model))
            .map(|listed| &listed.name)
    }

    /// A line for each harness it looked for, and each gateway it asked
    pub fn lines(&self) -> Vec<String> {
        let harnesses =
            Harness::ALL
                .into_iter()
                .map(|harness| match self.harnesses.contains(&harness) {
                    true => format!("{}: on the PATH", harness.command()),
                    false => format!("{}: not on the PATH", harness.command()),
                });
        let gateways = self.gateways.iter().map(|listed| {
            format!(
                "gateway {}: {}",
                listed.name,
                match &listed.why {
                    Some(why) => why.to_owned(),
                    None => format!("lists {} models", listed.models.len()),
                }
            )
        });
        harnesses.chain(gateways).collect()
    }
}

/// A row `init` writes, and the server its model runs on
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// The row it writes
    pub row: Row,
    /// The gateway or URL its model runs on, for a harness that needs one
    pub server: Option<ModelHost>,
}

/// What `--detect` binds
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bindings {
    /// The rows it writes, in the order it writes them
    pub choices: Vec<Choice>,
    /// A line for each role no harness got
    pub none: Vec<String>,
}

/// A file `init` wrote
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The file's name, without its `.md`
    pub file: &'static str,
    /// The row it wrote it from
    pub row: Row,
    /// The gateway its model runs behind, when one was named
    pub gateway: Option<String>,
}

impl fmt::Display for Written {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: wrote {} {} at {}",
            self.file,
            self.row.harness.as_str(),
            self.row.model,
            self.row.effort.as_str()
        )?;
        match &self.gateway {
            Some(gateway) => write!(f, ", through gateway {gateway}"),
            None => Ok(()),
        }
    }
}

/// A role `init` skipped, and what its harness lacks
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The role's name, or its baseline's
    pub name: &'static str,
    /// What the harness lacks
    pub why: String,
}

impl fmt::Display for Skipped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: skipped, {}", self.name, self.why)
    }
}

/// What `init` did to a folder
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Each file it wrote
    pub wrote: Vec<Written>,
    /// The name of each file it left alone because it is already there
    pub kept: Vec<&'static str>,
    /// Each role it skipped, with what its harness lacks
    pub skipped: Vec<Skipped>,
}

impl Report {
    /// A line for each file written, each one kept, and each role skipped
    pub fn lines(&self) -> Vec<String> {
        let wrote = self.wrote.iter().map(ToString::to_string);
        let kept = self
            .kept
            .iter()
            .map(|file| format!("{file}: kept, it is already there"));
        let skipped = self.skipped.iter().map(ToString::to_string);
        wrote.chain(kept).chain(skipped).collect()
    }

    /// The files it wrote for `role`
    pub fn names(&self, role: Role) -> Vec<&'static str> {
        self.wrote
            .iter()
            .filter(|written| written.row.role == role)
            .map(|written| written.file)
            .collect()
    }
}

/// The models file `text` holds, or why it cannot be read
///
/// # Errors
///
/// A message naming the part of [`FILE`] that names no harness, no role, no
/// baseline of that role, or holds a key a row cannot take.
pub fn read(text: &str) -> Result<File, String> {
    let table: toml::Table = text.parse().map_err(|e| format!("{FILE}: {e}"))?;
    let order = match table.get("order") {
        Some(toml::Value::Array(names)) => names
            .iter()
            .map(|name| {
                let name = name
                    .as_str()
                    .ok_or_else(|| format!("{FILE}: `order` holds harness names"))?;
                named(name)
                    .ok_or_else(|| format!("{FILE}: `order` names {name}, which is no harness"))
            })
            .collect::<Result<Vec<Harness>, String>>()?,
        _ => return Err(format!("{FILE}: it needs an `order` line of harness names")),
    };
    let mut rows = Vec::new();
    for (name, value) in table {
        if name == "order" {
            continue;
        }
        let harness = named(&name)
            .ok_or_else(|| format!("{FILE}: `{name}` is no harness: {}", list(&harness_names())))?;
        let roles = value
            .as_table()
            .ok_or_else(|| format!("{FILE}: `[{name}]` is no table of roles"))?;
        for (role, value) in roles {
            let Some(bound) = Role::named(role) else {
                return Err(format!(
                    "{FILE}: `[{name}.{role}]` is no role: {}",
                    list(&role_names())
                ));
            };
            let own = baseline::of_role(bound);
            match own.as_slice() {
                [only] => rows.push(binding(harness, bound, Some(only.name), value)?),
                [] => rows.push(binding(harness, bound, None, value)?),
                many => {
                    let baselines = value.as_table().ok_or_else(|| {
                        format!(
                            "{FILE}: `[{name}.{role}]` names each of its baselines: {}",
                            list(&names(many))
                        )
                    })?;
                    for (given, value) in baselines {
                        let Some(found) = many.iter().find(|own| own.name == given) else {
                            return Err(format!(
                                "{FILE}: `[{name}.{role}.{given}]` is no baseline of {role}: {}",
                                list(&names(many))
                            ));
                        };
                        rows.push(binding(harness, bound, Some(found.name), value)?);
                    }
                }
            }
        }
    }
    Ok(File { order, rows })
}

/// Writes a bound file for each `choices` row in `folder`, never over a file
///
/// Every text is read as the loader reads a file before any is written, so a
/// row the loader refuses changes nothing.
///
/// # Errors
///
/// A message naming the row the loader refuses, or the folder or file it could
/// not write.
pub fn write(folder: &Path, choices: &[Choice]) -> Result<Report, String> {
    let mut texts = Vec::new();
    for choice in choices {
        let text = choice.row.text(choice.server.as_ref());
        baseline::check(choice.row.file(), &text)
            .map_err(|e| format!("{}: {e}", choice.row.file()))?;
        texts.push(text);
    }
    if !choices.is_empty() {
        fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    let mut report = Report::default();
    for (choice, text) in choices.iter().zip(texts) {
        let path = folder.join(format!("{}.md", choice.row.file()));
        match write_new(&path, &text).map_err(|e| e.to_string())? {
            true => report.wrote.push(Written {
                file: choice.row.file(),
                gateway: match &choice.server {
                    Some(ModelHost::Gateway(gateway)) => Some(gateway.to_string()),
                    _ => None,
                },
                row: choice.row.clone(),
            }),
            false => report.kept.push(choice.row.file()),
        }
    }
    Ok(report)
}

/// Each role `harness` cannot serve, with what it lacks
pub fn skipped(harness: Harness) -> Vec<Skipped> {
    slots()
        .into_iter()
        .filter(|(role, _)| !harness.serves(*role))
        .map(|(role, baseline)| Skipped {
            name: baseline.unwrap_or(role.as_str()),
            why: lacks(role, harness),
        })
        .collect()
}

// The rows `init` writes a file for: each baseline of each role, and the role
// alone where it has none.
fn slots() -> Vec<(Role, Option<&'static str>)> {
    Role::ALL
        .into_iter()
        .flat_map(|role| {
            let own = baseline::of_role(role);
            match own.as_slice() {
                [] => vec![(role, None)],
                own => own.iter().map(|b| (role, Some(b.name))).collect(),
            }
        })
        .collect()
}

// Why `harness` cannot serve `role`, naming what it lacks.
fn lacks(role: Role, harness: Harness) -> String {
    match role
        .needs()
        .iter()
        .copied()
        .find(|need| !need.met_by(harness))
    {
        Some(need) => format!("{} lacks {}", harness.as_str(), need.description()),
        None => format!(
            "{} cannot serve the {} role",
            harness.as_str(),
            role.as_str()
        ),
    }
}

// The harness `name` names, if kelpie runs one.
fn named(name: &str) -> Option<Harness> {
    Harness::ALL.into_iter().find(|h| h.as_str() == name)
}

// The row `harness` runs `role`'s `baseline` on, held in the table `value`.
fn binding(
    harness: Harness,
    role: Role,
    baseline: Option<&'static str>,
    value: &toml::Value,
) -> Result<Row, String> {
    let named = role.as_str();
    let where_ = match baseline.filter(|given| *given != named) {
        Some(baseline) => format!("`[{}.{named}.{baseline}]`", harness.as_str()),
        None => format!("`[{}.{named}]`", harness.as_str()),
    };
    let table = value
        .as_table()
        .ok_or_else(|| format!("{FILE}: {where_} is no table of `model` and `effort`"))?;
    for key in table.keys() {
        if !KEYS.contains(&key.as_str()) {
            return Err(format!(
                "{FILE}: {where_} takes no `{key}`: it takes {}",
                list(&KEYS)
            ));
        }
    }
    let model = table
        .get("model")
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("{FILE}: {where_} needs `model`"))?
        .to_owned();
    let effort = table
        .get("effort")
        .and_then(|value| value.as_str())
        .and_then(Effort::parse)
        .ok_or_else(|| format!("{FILE}: {where_} needs `effort` as {}", list(&EFFORTS)))?;
    let second_look = match table.get("second_look") {
        Some(value) => value
            .as_bool()
            .ok_or_else(|| format!("{FILE}: {where_}: `second_look` is true or false"))?,
        None => false,
    };
    if second_look && role != Role::Reviewer {
        return Err(format!(
            "{FILE}: {where_}: `second_look` is a reviewer's, and {named} is no reviewer"
        ));
    }
    let context = match table.get("context") {
        Some(value) => {
            let tokens = value
                .as_integer()
                .ok_or_else(|| format!("{FILE}: {where_}: `context` is a number of tokens"))?;
            let size = ContextSize::try_from(tokens)
                .map_err(|e| format!("{FILE}: {where_}: `context` {e}"))?;
            Some(size.get())
        }
        None => None,
    };
    match (harness.facts().needs_model_server, context) {
        (true, None) => {
            return Err(format!(
                "{FILE}: {where_} needs `context`, since {} runs its model on a server",
                harness.as_str()
            ));
        }
        (false, Some(_)) => {
            return Err(format!(
                "{FILE}: {where_} takes no `context`: {} runs no model server",
                harness.as_str()
            ));
        }
        _ => {}
    }
    Ok(Row {
        harness,
        role,
        baseline,
        model,
        effort,
        second_look,
        context,
    })
}

fn harness_names() -> Vec<&'static str> {
    Harness::ALL.map(|harness| harness.as_str()).to_vec()
}

fn role_names() -> Vec<&'static str> {
    Role::ALL.map(|role| role.as_str()).to_vec()
}

fn names(baselines: &[&'static baseline::Baseline]) -> Vec<&'static str> {
    baselines.iter().map(|b| b.name).collect()
}

fn list(items: &[&str]) -> String {
    items.join(", ")
}
