//! The representative's one call: facts in, a post to a contributor out
//!
//! Kelpie's code decides what a post says and drafts the facts. The
//! representative, an agent file of its own (`agents.representative`), puts
//! them into words, in one voice for every post. A call is always a fresh
//! session with no tools and no folder to read, so it reads and runs nothing:
//! its whole input is the prompt. Nothing here posts. The caller does, and
//! applies its own rule when this fails.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use super::Runner;
use super::untrusted::fenced;
use crate::ports::{AgentCall, AgentError, Cost, Ending, Role, Session, Tools};
use crate::settings::SettingsError;
use crate::usage::{CallKind, Draft, Line, Spent, ended};
use crate::work_item::new_session_id;

/// What one post is made from
#[derive(Debug, Clone, Copy)]
pub struct Post<'a> {
    /// The pull request the post goes on, which the call is recorded against
    pub pull_request: u64,
    /// The kind of post, as a short instruction from the caller
    pub kind: &'a str,
    /// The facts the caller drafted, which are all the post may state
    pub facts: &'a str,
    /// The ids of the representative's own earlier comments on the pull
    /// request, read again through the forge so the call can avoid repeating them
    pub earlier: &'a [u64],
    /// The contributor's login, the one `@login` the post may leave live
    pub contributor: &'a str,
    /// What the contributor wrote, which is data and never instructions
    pub said: &'a str,
}

/// Why a post could not be worded
#[derive(Debug)]
pub enum RepresentError {
    /// `agents.representative` names no usable agent
    Agent(SettingsError),
    /// The call could not be set up: its session id, folder or instructions
    Setup(String),
    /// The call failed or was ended
    Call(AgentError),
    /// The call answered with nothing
    Empty,
}

impl fmt::Display for RepresentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Agent(e) => write!(f, "{e}"),
            Self::Setup(why) => write!(f, "cannot set up the representative's call: {why}"),
            Self::Call(e) => write!(f, "the representative's call failed: {e}"),
            Self::Empty => f.write_str("the representative answered with nothing"),
        }
    }
}

impl core::error::Error for RepresentError {}

impl Runner {
    /// The post `post` makes, in the representative's words, with every
    /// `@login` but the contributor's put in backticks so it pings nobody
    ///
    /// The call is made now and waits for its answer. Its spend goes to the
    /// usage ledger under the `representative` role.
    ///
    /// # Errors
    ///
    /// [`RepresentError`] when the agent is not usable, the call cannot be
    /// set up, fails or is ended, or answers with nothing.
    pub fn represent(&mut self, post: &Post<'_>) -> Result<String, RepresentError> {
        let agent = (self.settings.representative(&self.book)).map_err(RepresentError::Agent)?;
        let folder = &self.paths.representative;
        fs::create_dir_all(folder).map_err(|e| {
            RepresentError::Setup(format!("cannot create {}: {e}", folder.display()))
        })?;
        let session = new_session_id().map_err(|e| RepresentError::Setup(e.to_string()))?;
        let instructions = (self.paths.worker).join("representative-instructions.md");
        let text = agent.prompt.as_deref().unwrap_or_default();
        super::turn::write(&self.paths.worker, &instructions, text)
            .map_err(RepresentError::Setup)?;
        let call = AgentCall {
            issue: post.pull_request,
            settings: (self.paths.worker).join("representative-settings.json"),
            instructions: Some(instructions),
            ..AgentCall::new(
                Role::Representative,
                Tools::Answer,
                agent.model.harness.clone(),
                agent.model.model.as_str().to_owned(),
                agent.model.effort,
                Session::New(session),
                folder.clone(),
                self.post_prompt(post),
            )
        };
        let call = self.prepared(call).map_err(RepresentError::Setup)?;
        let started = self.ports.clock.now();
        let ran = (self.ports.agents)
            .prepare(&call)
            .and_then(|()| self.ports.agents.run(&call, &Ending::default()));
        if let Some(note) = unremoved(&call.settings) {
            self.notes.push(note);
        }
        if let Some(ended) = ended(&ran) {
            let mut draft = Draft::of(&call, agent.name.as_str(), CallKind::Post, started);
            draft.pull_request = Some(post.pull_request);
            let spent = ran.as_ref().ok().map(Spent::from);
            // A fresh session, so the call cost all of it.
            let now = self.ports.clock.now();
            let line = draft.line(now, ended, spent, Cost(0), self.pacer_lines());
            self.ledger.append(&Line::Call(line));
        }
        let reply = ran.map_err(RepresentError::Call)?;
        let words = reply.text.trim();
        match words.is_empty() {
            true => Err(RepresentError::Empty),
            false => Ok(neutered(words, post.contributor)),
        }
    }

    // What the call is asked: the kind of post, the facts, the earlier posts
    // that can still be read, and the contributor's text, the last two fenced
    // as data.
    fn post_prompt(&self, post: &Post<'_>) -> String {
        let mut prompt = format!("Kind of post: {}\n\nFacts:\n{}\n", post.kind, post.facts);
        let earlier: Vec<String> = (post.earlier.iter())
            .filter_map(|&id| self.ports.forge.comment_marks(&self.remote, id).ok())
            .map(|marks| marks.body)
            .collect();
        if !earlier.is_empty() {
            let what = "Your earlier posts on this pull request, oldest first,";
            prompt.push_str(&format!(
                "\n{}\n",
                fenced(what, &earlier.join("\n\n---\n\n"))
            ));
        }
        let what = format!("What @{} wrote", post.contributor);
        prompt.push_str(&format!(
            "\n{}\n\nWrite the post.",
            fenced(&what, post.said)
        ));
        prompt
    }
}

// What to note when the call's settings file stays: a file the call never
// wrote is not a failure, but one that cannot be removed is left behind
fn unremoved(settings: &Path) -> Option<String> {
    match fs::remove_file(settings) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Some(format!("cannot remove {}: {e}", settings.display()))
        }
        _ => None,
    }
}

// `text` with every `@login` that is not `contributor`'s inside backticks.
// Code the reply already has is left as it is: a fenced block that closes,
// and an inline span whose closing backticks are there. Backticks that close
// nothing are plain text, so a mention after them is still put in code, and
// so is one in a fence that never closes, which a reader may not see as code.
fn neutered(text: &str, contributor: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut paragraph = String::new();
    // The open fence's mark and length, and its lines so far
    let mut fence: Option<(char, usize, String)> = None;
    for line in text.split_inclusive('\n') {
        if let Some((mark, len, held)) = fence.as_mut() {
            held.push_str(line);
            if closes_fence(line, *mark, *len) {
                out.push_str(held);
                fence = None;
            }
            continue;
        }
        if let Some((mark, len)) = opens_fence(line) {
            out.push_str(&outside_spans(&paragraph, contributor));
            paragraph.clear();
            fence = Some((mark, len, line.to_owned()));
            continue;
        }
        paragraph.push_str(line);
        if line.trim().is_empty() {
            out.push_str(&outside_spans(&paragraph, contributor));
            paragraph.clear();
        }
    }
    if let Some((_, _, held)) = fence {
        paragraph.push_str(&held);
    }
    out.push_str(&outside_spans(&paragraph, contributor));
    out
}

// The mark and length of the fence `line` opens, if it opens one
fn opens_fence(line: &str) -> Option<(char, usize)> {
    let line = line.trim_start();
    let mark = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = line.chars().take_while(|&c| c == mark).count();
    (len >= 3).then_some((mark, len))
}

// Whether `line` ends a fence of `len` `mark`s
fn closes_fence(line: &str, mark: char, len: usize) -> bool {
    let line = line.trim();
    line.chars().count() >= len && line.chars().all(|c| c == mark)
}

// `paragraph` with its mentions outside inline code put in backticks
fn outside_spans(paragraph: &str, contributor: &str) -> String {
    let chars: Vec<char> = paragraph.chars().collect();
    let mut out = String::with_capacity(paragraph.len());
    let mut at = 0;
    while at < chars.len() {
        let here = chars[at];
        if here == '`' {
            let run = backticks(&chars, at);
            let end = closing_run(&chars, at + run, run).unwrap_or(at + run);
            out.extend(&chars[at..end]);
            at = end;
        } else if let Some(len) = link(&chars[at..]) {
            out.extend(&chars[at..at + len]);
            at += len;
        } else if here == '@'
            && can_ping(&chars, at)
            && let Some(len) = mention(&chars[at..])
        {
            let name: String = chars[at..at + len].iter().collect();
            match name[1..].eq_ignore_ascii_case(contributor) {
                true => out.push_str(&name),
                false => out.push_str(&format!("`{name}`")),
            }
            at += len;
        } else {
            out.push(here);
            at += 1;
        }
    }
    out
}

// The length of the web address that starts `chars`, up to the next space: an
// `@` in one is a part of the address, and wrapping it would break the link
fn link(chars: &[char]) -> Option<usize> {
    let starts = |scheme: &str| {
        let head: String = chars.iter().take(scheme.len()).collect();
        head.eq_ignore_ascii_case(scheme)
    };
    (starts("https://") || starts("http://"))
        .then(|| chars.iter().take_while(|c| !c.is_whitespace()).count())
}

// How many backticks run from `at`
fn backticks(chars: &[char], at: usize) -> usize {
    chars[at..].iter().take_while(|&&c| c == '`').count()
}

// Where the run of exactly `run` backticks that closes a span opened before
// `from` ends, if there is one
fn closing_run(chars: &[char], from: usize, run: usize) -> Option<usize> {
    let mut at = from;
    while at < chars.len() {
        if chars[at] != '`' {
            at += 1;
            continue;
        }
        let len = backticks(chars, at);
        if len == run {
            return Some(at + len);
        }
        at += len;
    }
    None
}

// Whether an `@` at `at` could ping: an email's or an escaped one does not
fn can_ping(chars: &[char], at: usize) -> bool {
    match at.checked_sub(1).map(|before| chars[before]) {
        Some(before) => !before.is_alphanumeric() && before != '\\',
        None => true,
    }
}

// The length of the mention that starts with the `@` in `chars`: a login, a
// bot's `[bot]`, or an `org/team`, or `None` when no name follows
fn mention(chars: &[char]) -> Option<usize> {
    let name = |c: &char| c.is_ascii_alphanumeric() || *c == '-';
    let mut len = 1 + chars[1..].iter().take_while(|c| name(c)).count();
    while len > 1 && chars[len - 1] == '-' {
        len -= 1;
    }
    if len == 1 {
        return None;
    }
    let rest: String = chars[len..].iter().take(5).collect();
    if rest == "[bot]" {
        return Some(len + 5);
    }
    if chars.get(len) == Some(&'/') {
        let team = chars[len + 1..]
            .iter()
            .take_while(|c| name(c) || **c == '_')
            .count();
        if team > 0 {
            return Some(len + 1 + team);
        }
    }
    Some(len)
}

#[cfg(test)]
mod tests;
