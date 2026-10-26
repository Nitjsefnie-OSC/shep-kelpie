//! `shep kelpie agents new` and `agents list`: bind a role's baseline, and show what is bound
//!
//! `new` writes the agent file that binds a baseline to a harness, model and
//! effort, after the loader has read the text. `list` shows every agent, then
//! each baseline no file stands for.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::agents::baseline::{self, Baseline};
use crate::agents::{Agents, Role, Runs};
use crate::settings::AgentName;
use crate::shep_home;

#[cfg(test)]
mod tests;

/// What `shep kelpie agents` takes
pub const USAGE: &str = "usage: shep kelpie agents new <role> --harness <harness> --model <model> --effort <effort> [--baseline <name>] [--name <file name>] [--second-look] [--gateway <name> | --url <url>] [--context <tokens>]\n       shep kelpie agents list";

/// Runs `shep kelpie agents ...`
pub fn main(args: &[String]) -> ExitCode {
    let ran = (|| {
        let shep_home = shep_home::required(shep_home::FLOCK_FIX)?;
        let folder = crate::home::kelpie_home_of(&shep_home).join(crate::agents::FOLDER);
        run(&folder, args)
    })();
    match ran {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("shep kelpie agents: {message}");
            ExitCode::FAILURE
        }
    }
}

// The lines `agents <args>` prints, with `folder` as kelpie's agents folder.
fn run(folder: &Path, args: &[String]) -> Result<Vec<String>, String> {
    match args {
        [verb, rest @ ..] if verb == "new" => new(folder, rest),
        [verb] if verb == "list" => list(folder),
        _ => Err(USAGE.to_owned()),
    }
}

// What `agents new <role> ...` was asked to write.
struct Ask {
    role: Role,
    baseline: Option<String>,
    name: Option<String>,
    harness: String,
    model: String,
    effort: String,
    second_look: bool,
    gateway: Option<String>,
    url: Option<String>,
    context: Option<u64>,
}

impl Ask {
    fn read(args: &[String]) -> Result<Self, String> {
        let [role, flags @ ..] = args else {
            return Err(USAGE.to_owned());
        };
        let role = Role::named(role).ok_or_else(|| format!("`{role}` is not a role"))?;
        let (mut baseline, mut name, mut harness, mut model, mut effort) =
            (None, None, None, None, None);
        let (mut second_look, mut gateway, mut url, mut context) = (false, None, None, None);
        let mut flags = flags.iter();
        while let Some(flag) = flags.next() {
            if flag == "--second-look" {
                second_look = true;
                continue;
            }
            let slot = match flag.as_str() {
                "--baseline" => &mut baseline,
                "--name" => &mut name,
                "--harness" => &mut harness,
                "--model" => &mut model,
                "--effort" => &mut effort,
                "--gateway" => &mut gateway,
                "--url" => &mut url,
                "--context" => &mut context,
                _ => return Err(format!("`{flag}` is not a flag of `agents new`")),
            };
            match flags.next().filter(|value| !value.starts_with("--")) {
                Some(value) => *slot = Some(value.clone()),
                None => return Err(format!("`{flag}` needs a value")),
            }
        }
        let need = |value: Option<String>, flag: &str| {
            value.ok_or_else(|| format!("`agents new` needs `{flag}`"))
        };
        if gateway.is_some() && url.is_some() {
            return Err("`--gateway` and `--url` name the same thing: give one".to_owned());
        }
        let context =
            match context {
                Some(tokens) => Some(tokens.parse().map_err(|_| {
                    format!("`--context` takes a number of tokens, not `{tokens}`")
                })?),
                None => None,
            };
        Ok(Self {
            role,
            baseline,
            name,
            harness: word(need(harness, "--harness")?, "--harness")?,
            model: need(model, "--model")?,
            effort: word(need(effort, "--effort")?, "--effort")?,
            second_look,
            gateway,
            url,
            context,
        })
    }

    // The baseline the file extends: None for an implementer, which has none.
    fn baseline(&self) -> Result<Option<&'static Baseline>, String> {
        let own = baseline::of_role(self.role);
        let names = || own.iter().map(|b| b.name).collect::<Vec<_>>().join(", ");
        let role = self.role.as_str();
        match (&self.baseline, own.as_slice()) {
            (Some(given), []) => Err(format!("the {role} role has no baseline, so not `{given}`")),
            (None, []) => Ok(None),
            (Some(given), _) => match own.iter().find(|b| b.name == given) {
                Some(found) => Ok(Some(found)),
                None => Err(format!(
                    "`{given}` is not a baseline of the {role} role: its baselines are {}",
                    names()
                )),
            },
            (None, [only]) => Ok(Some(only)),
            (None, _) => Err(format!(
                "the {role} role has several baselines: give one with `--baseline`: {}",
                names()
            )),
        }
    }

    // The file's name, and its text.
    fn file(&self) -> Result<(String, String), String> {
        let baseline = self.baseline()?;
        let name = match (&self.name, baseline) {
            (Some(name), _) => name.clone(),
            (None, Some(baseline)) => baseline.name.to_owned(),
            (None, None) => return Err("an implementer has no baseline: give `--name`".to_owned()),
        };
        AgentName::try_from(name.clone()).map_err(|e| e.to_string())?;
        let mut text = match baseline {
            Some(baseline) => format!("---\nextends: kelpie:{}\n", baseline.name),
            None => format!("---\nrole: {}\n", self.role.as_str()),
        };
        text.push_str(&format!(
            "harness: {}\nmodel: {}\neffort: {}\n",
            self.harness,
            quoted(&self.model),
            self.effort
        ));
        if self.second_look {
            text.push_str("second_look: true\n");
        }
        if let Some(gateway) = &self.gateway {
            text.push_str(&format!("gateway: {}\n", quoted(gateway)));
        }
        if let Some(url) = &self.url {
            text.push_str(&format!("url: {}\n", quoted(url)));
        }
        if let Some(tokens) = self.context {
            text.push_str(&format!("context: {tokens}\n"));
        }
        text.push_str("---\n");
        Ok((name, text))
    }
}

// `value` as a YAML string: as it stands when that reads back the same, else in quotes.
fn quoted(value: &str) -> String {
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

// `value` if it is one lowercase word, since it is written as it stands.
fn word(value: String, flag: &str) -> Result<String, String> {
    match value
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        true => Ok(value),
        false => Err(format!("`{flag}` takes one lowercase word, not `{value}`")),
    }
}

fn new(folder: &Path, args: &[String]) -> Result<Vec<String>, String> {
    let (name, text) = Ask::read(args)?.file()?;
    baseline::check(&name, &text)?;
    let path = folder.join(format!("{name}.md"));
    let stub = path.exists() && is_unbound(folder, &path)?;
    fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut file = OpenOptions::new();
    file.write(true);
    match stub {
        true => file.truncate(true),
        false => file.create_new(true),
    };
    let written = file
        .open(&path)
        .and_then(|mut f| f.write_all(text.as_bytes()));
    written.map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => already(&name),
        _ => format!("{}: {e}", path.display()),
    })?;
    Ok(vec![format!("wrote {name}.md")])
}

fn already(name: &str) -> String {
    format!(
        "`{name}.md` already exists, and `agents new` writes over only a file that binds nothing"
    )
}

// Whether the file at `path` is one the loader leaves out for binding nothing.
fn is_unbound(folder: &Path, path: &PathBuf) -> Result<bool, String> {
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let book = Agents::load(folder).map_err(|e| e.to_string())?;
    match book.unbound().iter().any(|u| &u.path == path) {
        true => Ok(true),
        false => Err(already(name)),
    }
}

fn list(folder: &Path) -> Result<Vec<String>, String> {
    let book = Agents::load(folder).map_err(|e| e.to_string())?;
    let mut lines = Vec::new();
    for (name, agent) in book.iter() {
        let role = agent.role.as_str();
        lines.push(match &agent.runs {
            Runs::Session { model, .. } => format!(
                "{name}: {role}, {}, {}, {}",
                model.harness.harness().as_str(),
                model.model.as_str(),
                model.effort.as_str()
            ),
            Runs::Command(_) => format!("{name}: {role}, command"),
            Runs::Endpoint(_) => format!("{name}: {role}, endpoint"),
            Runs::Bot(_) => format!("{name}: {role}, review bot"),
        });
    }
    for unbound in book.unbound() {
        lines.push(format!(
            "{}: {}, unbound: run `{}`",
            unbound.name,
            unbound.role.as_str(),
            unbound.command()
        ));
    }
    for baseline in baseline::ALL {
        let Some(role) = baseline.role() else {
            continue;
        };
        let file = AgentName::try_from(baseline.name.to_owned()).map_err(|e| e.to_string())?;
        if book.get(&file).is_none() && book.unbound().iter().all(|u| u.name != file) {
            let command = baseline::new_command(role, Some(baseline.name), baseline.name);
            lines.push(format!(
                "{}: baseline, no file: run `{command}`",
                baseline.name
            ));
        }
    }
    Ok(lines)
}
