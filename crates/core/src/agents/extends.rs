//! `extends`: an agent file that builds on another
//!
//! A file with `extends: <name>` starts from that agent's frontmatter and
//! body. The keys it sets replace the parent's, and a file with no body takes
//! the parent's. A file with `body: append` puts its body after the parent's
//! in place of replacing it. The merge is on the text, before the typed parse, so every
//! check on a plain file applies to the merged one.

use super::front;

/// A file's text with its parent merged in
///
/// The file's own frontmatter lines stay where they are, and the keys it
/// inherits follow them, so a refusal's line in the merged text is the
/// file's own line for a key the file sets.
#[derive(Debug)]
pub(super) struct Merged {
    /// The text to parse
    pub(super) text: String,
    // The last line of the file's own frontmatter, and the parent it
    // inherits through. None for a file with no `extends`.
    inherits: Option<(usize, String)>,
}

impl Merged {
    /// `message`, a refusal of `text`, with a line past the file's own
    /// frontmatter replaced by the parent the key came through
    pub(super) fn place(&self, message: String) -> String {
        let Some((own_last, parent)) = &self.inherits else {
            return message;
        };
        let Some((head, at)) = message.rsplit_once(" at line ") else {
            return message;
        };
        let line = at
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|n| n.parse::<usize>().ok());
        match line {
            Some(line) if line > *own_last => {
                format!("{head} inherited through `{parent}`")
            }
            _ => message,
        }
    }
}

/// `text` with its parent merged in, if it has an `extends` key
///
/// `parent` gives the text of the agent a name stands for, or the refusal
/// for a name that stands for none. `own` is the name of the file `text` is from, which a chain
/// back to it is refused as a cycle. Text with no `extends` comes back as it
/// is.
pub(super) fn merged<'a>(
    own: &str,
    text: &str,
    parent: &dyn Fn(&str) -> Result<ParentText<'a>, String>,
) -> Result<Merged, String> {
    let mut chain = vec![own.to_owned()];
    let (text, inherits) = resolve(text, None, parent, &mut chain)?;
    Ok(Merged { text, inherits })
}

/// What an `extends` name starts with to name one of kelpie's own agents, built
/// into the binary, in place of a file
pub(super) const SHIPPED: &str = "kelpie:";

/// The name of the shipped agent `text`'s own `extends` key names, if it names one
pub(super) fn shipped_parent(text: &str) -> Option<String> {
    let (front, _) = front::split(text)?;
    let extended = entries(front).into_iter().find(|e| e.key == "extends")?;
    extended.value().strip_prefix(SHIPPED).map(str::to_owned)
}

/// The text of a parent: a file, which may extend another, or a default
#[derive(Debug, Clone, Copy)]
pub(super) enum ParentText<'a> {
    File(&'a str),
    Default(&'a str),
}

// The merged text, and for a file with `extends` the last line of its own
// frontmatter and the parent it inherits through.
type Resolved = (String, Option<(usize, String)>);

// `from` is the name of the parent file `text` is from, None for the file
// being loaded. A refusal of a parent's own text names it, since its line is
// no line of the file being loaded.
fn resolve<'a>(
    text: &str,
    from: Option<&str>,
    parent: &dyn Fn(&str) -> Result<ParentText<'a>, String>,
    chain: &mut Vec<String>,
) -> Result<Resolved, String> {
    let Some((front, body)) = front::split(text) else {
        return Ok((text.to_owned(), None));
    };
    let child = entries(front);
    let Some(extended) = child.iter().find(|e| e.key == "extends") else {
        return Ok((text.to_owned(), None));
    };
    let refuse = |message: String| match from {
        Some(file) => format!("in `{file}.md`: {message}"),
        None => message,
    };
    let append = appends(&child).map_err(refuse)?;
    let name = extended.value();
    if chain.contains(&name) {
        let path: Vec<&str> = chain.iter().map(String::as_str).chain([&*name]).collect();
        return Err(refuse(format!(
            "`extends` runs in a circle: {}",
            path.join(" extends ")
        )));
    }
    let parent_text = match parent(&name).map_err(refuse)? {
        ParentText::Default(text) => {
            let extends = front::split(text)
                .is_some_and(|(front, _)| entries(front).iter().any(|e| e.key == "extends"));
            if extends {
                return Err(refuse(format!(
                    "`extends: {name}` names one of kelpie's own agents that itself extends \
                     another, which is not supported: write a file `{name}.md` to extend"
                )));
            }
            text.to_owned()
        }
        ParentText::File(text) => {
            chain.push(name.clone());
            let resolved = resolve(text, Some(&name), parent, chain);
            chain.pop();
            resolved?.0
        }
    };
    let (parent_front, parent_body) = front::split(&parent_text).ok_or_else(|| {
        refuse(format!(
            "`extends: {name}` names a file with no frontmatter"
        ))
    })?;
    let mut keys = own_lines(front);
    // The file's frontmatter plus the opening `---`, as the lines it spans.
    let own_last = 1 + keys.lines().count();
    for entry in entries(parent_front) {
        if !child.iter().any(|c| c.key == entry.key) {
            keys.push_str(&entry.text);
        }
    }
    let body = match (body.trim().is_empty(), append) {
        (true, _) => parent_body.to_owned(),
        (false, false) => body.to_owned(),
        (false, true) if parent_body.trim().is_empty() => body.to_owned(),
        (false, true) => format!(
            "{}\n\n{}",
            parent_body.trim_end(),
            body.trim_start_matches(['\n', '\r'])
        ),
    };
    Ok((format!("---\n{keys}---\n{body}"), Some((own_last, name))))
}

// Whether the file's `body` key says `append`. The key takes no other value,
// and the file sets it once.
fn appends(child: &[Entry]) -> Result<bool, String> {
    let mut bodies = child.iter().filter(|e| e.key == "body");
    let Some(body) = bodies.next() else {
        return Ok(false);
    };
    if let Some(again) = bodies.next() {
        return Err(format!(
            "`body` is set twice, which is not allowed, at line {}",
            again.line
        ));
    }
    match body.value().as_str() {
        "append" => Ok(true),
        other => Err(format!(
            "`body: {other}` is not a value `body` takes: only `append` is, at line {}",
            body.line
        )),
    }
}

// The file's frontmatter as written, with the lines of its `extends` and
// `body` keys left blank so every other line keeps its number.
fn own_lines(front: &str) -> String {
    let mut in_extends = false;
    let mut kept = String::new();
    for line in front.split_inclusive('\n') {
        if !line.starts_with([' ', '\t', '#', '-', '\r', '\n']) {
            in_extends = line
                .split_once(':')
                .is_some_and(|(k, _)| matches!(k.trim(), "extends" | "body"));
        }
        kept.push_str(if in_extends { "\n" } else { line });
    }
    if !kept.is_empty() && !kept.ends_with('\n') {
        kept.push('\n');
    }
    kept
}

// One top-level key of the frontmatter, with the lines under it: its
// continuation, comments and blank lines, up to the next key.
struct Entry {
    key: String,
    text: String,
    // The line of the file the key is on, which counts the opening `---`.
    line: usize,
}

impl Entry {
    // The text after `key:`, without a trailing comment or quotes.
    fn value(&self) -> String {
        let line = self.text.lines().next().unwrap_or("");
        let value = line.split_once(':').map_or("", |(_, v)| v);
        let value = value.trim();
        // A quoted value runs to its closing quote, so a ` #` inside is not a comment.
        for quote in ['"', '\''] {
            if let Some(rest) = value.strip_prefix(quote) {
                return rest.split(quote).next().unwrap_or("").to_owned();
            }
        }
        value.split(" #").next().unwrap_or("").trim().to_owned()
    }
}

// The frontmatter's keys in order. Lines before the first key are dropped.
fn entries(front: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for (at, line) in front.split_inclusive('\n').enumerate() {
        let key = key_of(line).map(str::to_owned);
        match (key, entries.last_mut()) {
            (Some(key), _) => entries.push(Entry {
                key,
                text: line.to_owned(),
                line: at + 2,
            }),
            (None, Some(last)) => last.text.push_str(line),
            (None, None) => {}
        }
    }
    for entry in &mut entries {
        if !entry.text.ends_with('\n') {
            entry.text.push('\n');
        }
    }
    entries
}

// The top-level key a frontmatter line starts, if it starts one.
fn key_of(line: &str) -> Option<&str> {
    (!line.starts_with([' ', '\t', '#', '-', '\r', '\n']))
        .then(|| line.split_once(':'))
        .flatten()
        .map(|(key, _)| key.trim())
}

/// The keys `text`'s own frontmatter sets, and where its `extends` line ends
#[derive(Debug)]
pub(super) struct Own {
    pub(super) keys: Vec<String>,
    /// The byte offset in `text` just past the `extends` line, if there is one
    pub(super) after_extends: Option<usize>,
}

/// What `text`'s own frontmatter sets, or none for text with no frontmatter
pub(super) fn own(text: &str) -> Option<Own> {
    let (front, _) = front::split(text)?;
    let start = front.as_ptr() as usize - text.as_ptr() as usize;
    let mut keys = Vec::new();
    let mut after_extends = None;
    let mut offset = start;
    for line in front.split_inclusive('\n') {
        offset += line.len();
        if let Some(key) = key_of(line) {
            if key == "extends" {
                after_extends = Some(offset);
            }
            keys.push(key.to_owned());
        }
    }
    Some(Own {
        keys,
        after_extends,
    })
}
