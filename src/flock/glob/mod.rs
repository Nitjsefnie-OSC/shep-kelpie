//! A `-p` value that is a glob over the projects' names, which only `set` and `get` take

use std::fmt;

use crate::home::ProjectName;

const GLOB_CHARS: [char; 3] = ['*', '?', '['];

/// What `-p` names: one project, or the projects whose names match a glob
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Named {
    /// A project's name, which holds none of `*`, `?` and `[`
    Project(ProjectName),
    /// A pattern over the registered projects' names
    Glob(Glob),
}

impl Named {
    /// `text` as a glob when it holds `*`, `?` or `[`, else as a project's name
    ///
    /// # Errors
    ///
    /// A message when `text` is no project's name, or a glob with a `[` that
    /// has no `]`.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.contains(GLOB_CHARS) {
            return Glob::new(text).map(Self::Glob);
        }
        ProjectName::try_from(text)
            .map(Self::Project)
            .map_err(|e| e.to_string())
    }

    /// The project, or the glob where `command` takes one
    ///
    /// # Errors
    ///
    /// A message naming `set` and `get` when `command` is another verb and
    /// this is a glob.
    pub fn split(
        named: Option<Self>,
        command: &str,
    ) -> Result<(Option<ProjectName>, Option<Glob>), String> {
        match named {
            None => Ok((None, None)),
            Some(Self::Project(name)) => Ok((Some(name), None)),
            Some(Self::Glob(glob)) if matches!(command, "set" | "get") => Ok((None, Some(glob))),
            Some(Self::Glob(glob)) => Err(format!(
                "`-p {glob}` is a glob, which only `set` and `get` take: name one project to `{command}`"
            )),
        }
    }
}

/// A pattern over project names: `*` for any run of characters, `?` for one,
/// `[abc]`, `[a-f]` and `[!abc]` for one of a set, matched case-sensitively
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    text: String,
    tokens: Vec<Token>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(char),
    One,
    Many,
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

impl Glob {
    fn new(text: &str) -> Result<Self, String> {
        let mut tokens = Vec::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            tokens.push(match c {
                '*' => Token::Many,
                '?' => Token::One,
                '[' => {
                    let negated = chars.next_if_eq(&'!').is_some();
                    let mut ranges = Vec::new();
                    loop {
                        let first = chars
                            .next()
                            .ok_or_else(|| format!("`[` has no `]` in the glob {text:?}"))?;
                        if first == ']' && !ranges.is_empty() {
                            break;
                        }
                        let mut ahead = chars.clone();
                        let last = match (ahead.next(), ahead.next()) {
                            (Some('-'), Some(last)) if last != ']' => {
                                chars = ahead;
                                last
                            }
                            _ => first,
                        };
                        ranges.push((first, last));
                    }
                    Token::Class { negated, ranges }
                }
                literal => Token::Literal(literal),
            });
        }
        Ok(Self {
            text: text.to_owned(),
            tokens,
        })
    }

    /// Whether the whole of `name` matches
    pub fn matches(&self, name: &str) -> bool {
        let name: Vec<char> = name.chars().collect();
        matches_from(&self.tokens, &name)
    }
}

impl fmt::Display for Glob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

fn matches_from(tokens: &[Token], name: &[char]) -> bool {
    match tokens.split_first() {
        None => name.is_empty(),
        Some((Token::Many, rest)) => (0..=name.len()).any(|skip| matches_from(rest, &name[skip..])),
        Some((token, rest)) => name
            .split_first()
            .is_some_and(|(c, tail)| token.accepts(*c) && matches_from(rest, tail)),
    }
}

impl Token {
    // One character; `Many` is the caller's.
    fn accepts(&self, c: char) -> bool {
        match self {
            Self::Literal(literal) => *literal == c,
            Self::One => true,
            Self::Many => false,
            Self::Class { negated, ranges } => {
                ranges.iter().any(|(from, to)| (*from..=*to).contains(&c)) != *negated
            }
        }
    }
}

#[cfg(test)]
mod tests;
