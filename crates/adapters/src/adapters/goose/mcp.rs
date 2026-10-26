//! The MCP servers in a goose call's flags
//!
//! goose reads no `.mcp.json`, so a call gets each server as a flag:
//! `--with-extension "<name>:KEY=value command args"` for stdio,
//! `--with-streamable-http-extension <url>` for HTTP, with no header or name. A
//! repeated name ends the run, so only a new session gets flags, and its keys
//! are recorded for a resumed call's hook. goose splits the stdio string like a
//! shell with no escape (`split_command_args`): a double quote cannot be
//! written, and a single quote counts only at a word's start. A server's tools
//! are `<key>__<tool>`, so the hook is handed each loaded server's key.

use std::collections::HashSet;
use std::ffi::OsString;

use serde_json::Value;

use super::files::Files;
use crate::ports::mcp::{SERVER, servers};
use crate::ports::{AgentCall, AgentError, Session, Tools};

/// goose's own extension, which a call that works runs under this name
const BUILTIN: &str = "developer";

/// The env keys goose never passes a server (`Envs::DISALLOWED_KEYS`), compared without case
const SKIPPED_ENV: [&str; 31] = [
    "PATH",
    "PATHEXT",
    "SystemRoot",
    "windir",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "LD_AUDIT",
    "LD_DEBUG",
    "LD_BIND_NOW",
    "LD_ASSUME_KERNEL",
    "DYLD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_FRAMEWORK_PATH",
    "PYTHONPATH",
    "PYTHONHOME",
    "NODE_OPTIONS",
    "RUBYOPT",
    "GEM_PATH",
    "GEM_HOME",
    "CLASSPATH",
    "GO111MODULE",
    "GOROOT",
    "APPINIT_DLLS",
    "SESSIONNAME",
    "ComSpec",
    "TEMP",
    "TMP",
    "LOCALAPPDATA",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
];

/// A server goose loads: the name its tools are prefixed with, and the flag that adds it
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Loaded {
    pub(super) key: String,
    flag: [String; 2],
}

/// The flags that add the call's servers to a new session, one per server, the browser and the code graph last
///
/// A resumed session is given none: it already holds the set it began with.
pub(super) fn flags(call: &AgentCall) -> Vec<OsString> {
    match call.session {
        Session::New(_) => loaded(call)
            .into_iter()
            .flat_map(|server| server.flag)
            .map(OsString::from)
            .collect(),
        Session::Resume(_) => Vec::new(),
    }
}

/// The keys of the servers a new session loads, one each
pub(super) fn keys(call: &AgentCall) -> Vec<String> {
    loaded(call).into_iter().map(|server| server.key).collect()
}

/// The keys of the servers the call's session holds
///
/// A new session holds what the call loads. A resumed one holds the set it was created with,
/// which kelpie recorded then, whatever the call loads now: goose keeps that set, so the hook
/// and the mode follow it. A session with no record holds none.
///
/// A retro is handed no server, so the hook lets it none through, though the worker's session it
/// resumes holds them: it stays unable to write.
pub(super) fn held(call: &AgentCall, files: &Files) -> Result<Vec<String>, AgentError> {
    match &call.session {
        Session::New(_) => Ok(keys(call)),
        Session::Resume(_) if call.tools == Tools::Retro => Ok(Vec::new()),
        Session::Resume(id) => Ok(files.servers(id)?.unwrap_or_default()),
    }
}

/// Whether the call's session holds any server
pub(super) fn loads(call: &AgentCall, files: &Files) -> bool {
    held(call, files).is_ok_and(|held| !held.is_empty())
}

// The servers goose can load, in the order of their flags. The browser and the code graph come
// last, the graph at the very end, and win a key they share with a repo server, whatever that
// server is named. goose ends the run over a key used twice, so the later of two other servers
// that share one is left out.
pub(super) fn loaded(call: &AgentCall) -> Vec<Loaded> {
    let mut all = servers(call);
    let mut ours = |present: bool, name: &str| {
        let entry = if present {
            all.remove(name)?
        } else {
            return None;
        };
        converted(name, &entry).ok()
    };
    let browser = ours(call.reach.browser.is_some(), crate::bridge::SERVER);
    let graph = ours(call.reach.code_graph.is_some(), SERVER);
    let mut taken: HashSet<String> = (graph.iter().chain(&browser))
        .map(|g| g.key.clone())
        .collect();
    let mut loaded: Vec<Loaded> = (all.into_iter())
        .filter_map(|(name, entry)| converted(&name, &entry).ok())
        .filter(|server| taken.insert(server.key.clone()))
        .collect();
    loaded.extend(browser);
    loaded.extend(graph);
    loaded
}

/// Why goose cannot load a Claude-shaped `mcpServers` entry, if it cannot
pub(crate) fn refusal(name: &str, entry: &Value) -> Option<String> {
    converted(name, entry).err()
}

/// The env keys of a stdio entry that goose loads without, if there are any
pub(crate) fn skipped_env(name: &str, entry: &Value) -> Option<String> {
    converted(name, entry).ok()?;
    let skipped: Vec<String> = (entry["env"].as_object().into_iter().flatten())
        .map(|(key, _)| key)
        .filter(|key| skips(key))
        .map(|key| format!("`{key}`"))
        .collect();
    (!skipped.is_empty()).then(|| {
        format!(
            "the env keys {}, which goose never passes a server",
            skipped.join(", ")
        )
    })
}

fn skips(key: &str) -> bool {
    SKIPPED_ENV.iter().any(|s| s.eq_ignore_ascii_case(key))
}

fn converted(name: &str, entry: &Value) -> Result<Loaded, String> {
    // A JSON `null` names nothing, as a missing field does.
    let has = |field: &str| !entry[field].is_null();
    match (entry["type"].as_str(), has("command"), has("url")) {
        (Some("stdio"), ..) | (None, true, _) => stdio(name, entry),
        (Some("http" | "streamable-http"), ..) | (None, false, true) => http(entry),
        (Some("sse"), ..) => Err(
            "goose has no SSE transport: the server may offer streamable HTTP, commonly at `/mcp`"
                .to_owned(),
        ),
        (Some(other), ..) => Err(format!("goose has no `{other}` transport")),
        (None, false, false) => Err("it names neither a `command` nor a `url`".to_owned()),
    }
}

fn stdio(name: &str, entry: &Value) -> Result<Loaded, String> {
    // goose takes a name of two or more of these, and keeps it in lower case.
    let named = name.chars().count() >= 2
        && (name.chars()).all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !named {
        return Err(
            "goose takes only two or more letters, digits, `_` and `-` in a server's name"
                .to_owned(),
        );
    }
    let key = name.to_lowercase();
    if key == BUILTIN {
        return Err("goose runs its own `developer` extension under that name".to_owned());
    }
    if has(entry, "cwd") {
        return Err("goose cannot start a server in a folder of its own".to_owned());
    }
    let command = text(entry, "command")?;
    if command.contains('=') {
        return Err("goose would read a `command` holding `=` as an env pair".to_owned());
    }
    let mut words = Vec::new();
    for (key, value) in object(entry, "env")? {
        let plain = !key.is_empty() && (key.chars()).all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !plain {
            return Err(format!(
                "its `env` key `{key}` is not a plain variable name"
            ));
        }
        if !skips(key) {
            words.push(word("env", &format!("{key}={value}"))?);
        }
    }
    words.push(word("command", command)?);
    for arg in list(entry, "args")? {
        if arg.is_empty() {
            return Err("goose drops an empty argument, which it cannot be given".to_owned());
        }
        words.push(word("args", arg)?);
    }
    Ok(Loaded {
        flag: [
            "--with-extension".to_owned(),
            format!("{name}:{}", words.join(" ")),
        ],
        key,
    })
}

fn http(entry: &Value) -> Result<Loaded, String> {
    let url = text(entry, "url")?;
    let no_headers = match &entry["headers"] {
        Value::Null => true,
        Value::Object(headers) => headers.is_empty(),
        _ => false,
    };
    if !no_headers {
        return Err(
            "goose's streamable HTTP flag takes no `headers`, so the server would be reached \
             without them"
                .to_owned(),
        );
    }
    word("url", url)?;
    let key = derived_key(url).ok_or_else(|| {
        "kelpie cannot tell the name goose would give a server at this `url`".to_owned()
    })?;
    if key == BUILTIN {
        return Err(
            "goose runs its own `developer` extension under the name this `url` gets".to_owned(),
        );
    }
    Ok(Loaded {
        flag: [
            "--with-streamable-http-extension".to_owned(),
            url.to_owned(),
        ],
        key,
    })
}

/// The name goose gives a streamable HTTP server at `url`, which is also its tools' prefix
///
/// goose names it from the URL's host, then `_<port>` when it is not the scheme's own, then
/// `_<path>`, and `name_to_key`s the whole. `None` for a URL kelpie does not model the
/// normalising of: another scheme, user info, an IPv6 or abbreviated IPv4 host, an escape or
/// dot segment in the path.
pub(super) fn derived_key(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let default = match scheme.to_ascii_lowercase().as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    if !rest.chars().all(|c| c.is_ascii_graphic()) || rest.contains('\\') {
        return None;
    }
    let (authority, after) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
    let path = after.split(['?', '#']).next().unwrap_or("");
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port),
        None => (authority, ""),
    };
    let port = match port {
        "" => None,
        digits if digits.chars().all(|c| c.is_ascii_digit()) => {
            Some(digits.parse::<u16>().ok()?).filter(|&p| p != default)
        }
        _ => return None,
    };
    let host = host.to_ascii_lowercase();
    let plain = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_');
    let last = host.rsplit('.').next().unwrap_or("");
    let numeric = last.starts_with(|c: char| c.is_ascii_digit());
    if !plain || (numeric && !is_dotted_quad(&host)) {
        return None;
    }
    let kept = |c: char| c.is_ascii_alphanumeric() || "-._~/%:@!$&'()*+,;=".contains(c);
    let path = path.trim_matches('/');
    let dotted = path.split('/').any(|segment| {
        let segment = segment.replace("%2e", ".").replace("%2E", ".");
        segment == "." || segment == ".."
    });
    if dotted || !path.chars().all(kept) {
        return None;
    }
    let mut name = host;
    if let Some(port) = port {
        name.push_str(&format!("_{port}"));
    }
    if !path.is_empty() {
        name.push('_');
        name.push_str(path);
    }
    Some(name_to_key(&name))
}

fn is_dotted_quad(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_digit())
                && part.parse::<u8>().is_ok()
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

// goose's `name_to_key`: letters, digits, `_` and `-` stay, white space goes, the rest is `_`.
fn name_to_key(name: &str) -> String {
    let kept = name.chars().filter(|c| !c.is_whitespace());
    kept.map(|c| match c {
        c if c.is_ascii_alphanumeric() || c == '_' || c == '-' => c,
        _ => '_',
    })
    .collect::<String>()
    .to_lowercase()
}

fn has(entry: &Value, field: &str) -> bool {
    !entry[field].is_null()
}

fn text<'a>(entry: &'a Value, field: &str) -> Result<&'a str, String> {
    let text = entry[field]
        .as_str()
        .ok_or_else(|| format!("its `{field}` is not a string"))?;
    if text.is_empty() {
        return Err(format!("its `{field}` is empty"));
    }
    Ok(text)
}

fn list<'a>(entry: &'a Value, field: &str) -> Result<Vec<&'a str>, String> {
    let Some(items) = entry.get(field).filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };
    let items = (items.as_array()).ok_or_else(|| format!("its `{field}` is not a list"))?;
    (items.iter())
        .map(|item| item.as_str())
        .collect::<Option<_>>()
        .ok_or_else(|| format!("its `{field}` holds a value that is not a string"))
}

fn object<'a>(entry: &'a Value, field: &str) -> Result<Vec<(&'a String, &'a str)>, String> {
    let Some(value) = entry.get(field).filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };
    let pairs = (value.as_object()).ok_or_else(|| format!("its `{field}` is not a table"))?;
    (pairs.iter())
        .map(|(key, value)| Some((key, value.as_str()?)))
        .collect::<Option<_>>()
        .ok_or_else(|| format!("its `{field}` holds a value that is not a string"))
}

// One word of the stdio string, as goose reads it back to the same text. It is grouped in double
// quotes when it holds white space or would start with a single quote, which goose reads as a
// group of its own. goose expands no `${VAR}`, and a double quote cannot be written.
fn word(field: &str, text: &str) -> Result<String, String> {
    if text.contains("${") {
        return Err(format!(
            "its `{field}` holds a `${{VAR}}`, which goose does not expand"
        ));
    }
    if text.contains('"') {
        return Err(format!(
            "its `{field}` holds a double quote, which goose's command string cannot write"
        ));
    }
    if text.contains(char::is_whitespace) || text.starts_with('\'') {
        return Ok(format!("\"{text}\""));
    }
    Ok(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_is_named_from_its_host_port_and_path() {
        for (url, key) in [
            ("https://docs.example.com/mcp", "docs_example_com_mcp"),
            ("https://Docs.Example.com", "docs_example_com"),
            ("http://localhost:8080/a/b/", "localhost_8080_a_b"),
            ("http://localhost:80/mcp?x=1#top", "localhost_mcp"),
            ("https://h.example.com:443", "h_example_com"),
            ("http://127.0.0.1:3000/v1.2/mcp", "127_0_0_1_3000_v1_2_mcp"),
        ] {
            assert_eq!(derived_key(url).as_deref(), Some(key), "{url}");
        }
        for url in [
            "ftp://h.example.com/x",
            "https://u@h.example.com",
            "http://[::1]:3000",
            "http://127.1/mcp",
            "https://h.example.com/a/../b",
            "https://h.example.com/a b",
            "https://h.example.com:99999",
            "no scheme",
        ] {
            assert_eq!(derived_key(url), None, "{url}");
        }
    }
}
