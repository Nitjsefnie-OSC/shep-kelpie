//! `extends`: an agent file that builds on another
//!
//! A file with `extends: <name>` starts from that agent's frontmatter and
//! body. The keys it sets replace the parent's, and a file with no body takes
//! the parent's. The merge is on the text, before the typed parse, so every
//! check on a plain file applies to the merged one.

use super::front;

/// `text` with its parent merged in, if it has an `extends` key
///
/// `parent` gives the text of the agent a name stands for, or None if kelpie
/// has none. `own` is the name of the file `text` is from, which a chain
/// back to it is refused as a cycle. Text with no `extends` comes back as it
/// is.
pub(super) fn merged<'a>(
    own: &str,
    text: &str,
    parent: &dyn Fn(&str) -> Option<ParentText<'a>>,
) -> Result<String, String> {
    let mut chain = vec![own.to_owned()];
    resolve(text, parent, &mut chain)
}

/// The text of a parent: a file, which may extend another, or a default
#[derive(Debug, Clone, Copy)]
pub(super) enum ParentText<'a> {
    File(&'a str),
    Default(&'a str),
}

fn resolve<'a>(
    text: &str,
    parent: &dyn Fn(&str) -> Option<ParentText<'a>>,
    chain: &mut Vec<String>,
) -> Result<String, String> {
    let Some((front, body)) = front::split(text) else {
        return Ok(text.to_owned());
    };
    let child = entries(front);
    let Some(extended) = child.iter().find(|e| e.key == "extends") else {
        return Ok(text.to_owned());
    };
    let name = extended.value();
    if chain.contains(&name) {
        let path: Vec<&str> = chain.iter().map(String::as_str).chain([&*name]).collect();
        return Err(format!(
            "`extends` runs in a circle: {}",
            path.join(" extends ")
        ));
    }
    let parent_text = match parent(&name) {
        None => {
            return Err(format!(
                "`extends: {name}` names no agent: write a file `{name}.md` or extend one of \
                 kelpie's own agents"
            ));
        }
        Some(ParentText::Default(text)) => {
            let extends = front::split(text)
                .is_some_and(|(front, _)| entries(front).iter().any(|e| e.key == "extends"));
            if extends {
                return Err(format!(
                    "`extends: {name}` names one of kelpie's own agents that itself extends \
                     another, which is not supported: write a file `{name}.md` to extend"
                ));
            }
            text.to_owned()
        }
        Some(ParentText::File(text)) => {
            chain.push(name);
            let resolved = resolve(text, parent, chain);
            chain.pop();
            resolved?
        }
    };
    let (parent_front, parent_body) = front::split(&parent_text).ok_or_else(|| {
        format!(
            "`extends: {}` names a file with no frontmatter",
            extended.value()
        )
    })?;
    let mut keys = String::new();
    for entry in entries(parent_front) {
        if !child.iter().any(|c| c.key == entry.key) {
            keys.push_str(&entry.text);
        }
    }
    for entry in child.iter().filter(|e| e.key != "extends") {
        keys.push_str(&entry.text);
    }
    let body = if body.trim().is_empty() {
        parent_body
    } else {
        body
    };
    Ok(format!("---\n{keys}---\n{body}"))
}

// One top-level key of the frontmatter, with the lines under it: its
// continuation, comments and blank lines, up to the next key.
struct Entry {
    key: String,
    text: String,
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
    for line in front.split_inclusive('\n') {
        let key = (!line.starts_with([' ', '\t', '#', '-', '\r', '\n']))
            .then(|| line.split_once(':'))
            .flatten()
            .map(|(key, _)| key.trim().to_owned());
        match (key, entries.last_mut()) {
            (Some(key), _) => entries.push(Entry {
                key,
                text: line.to_owned(),
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
