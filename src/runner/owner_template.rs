//! The pull request template a repo takes from its owner's `.github` repository
//!
//! GitHub offers a repo that has no template of its own the one in its owner's
//! `.github` repository. The worker cannot read that repository, so the runner
//! reads it through the forge as it starts and when it rereads its settings.
//! The worker's instructions only name the template: its text goes in a file
//! in the item's build folder, which the worker reads when it writes its body,
//! so a session is not made to carry the template on every request for a line
//! it writes once near the end.

use std::path::Path;

use serde::Serialize;

use super::Runner;
use super::instructions::{PLACES, Template, pick, template};
use crate::ports::{Forge, ForgeError, Visibility};
use crate::settings::ForgeSlug;

/// How much template text the worker's file keeps, over which it is cut
const KEPT: usize = 8 * 1024;

/// Where a repo's pull request template comes from
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateSource {
    /// The repo's own
    Repo,
    /// The owner's `.github` repository, named here
    Owner(String),
    /// None: the worker writes the body with the `pr` skill
    None,
}

impl std::fmt::Display for TemplateSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Repo => f.write_str("the repo's own"),
            Self::Owner(repo) => write!(f, "the owner's, from {repo}"),
            Self::None => f.write_str("none"),
        }
    }
}

/// A template read from the owner's `.github` repository
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OwnerTemplate {
    repo: ForgeSlug,
    found: Template,
    // Each file's name and text: one for a template, one per file of a folder
    texts: Vec<(String, String)>,
}

impl OwnerTemplate {
    /// The owner's `.github` repository the template came from
    pub(super) fn repo(&self) -> &ForgeSlug {
        &self.repo
    }

    /// What the worker is told of the template, naming the file that holds its text
    ///
    /// The text itself is not here: it goes in `file`, which the worker reads
    /// when it writes its body, so the instructions it carries every request
    /// stay short.
    pub(super) fn line(&self, file: &Path) -> String {
        let (repo, file) = (self.repo.as_str(), file.display());
        match &self.found {
            Template::File(path) => format!(
                "The repo has no pull request template of its own, so its owner's applies, \
                 `{path}` of `{repo}`. Its text is in `{file}`: read it before you write the \
                 body, fill it in rather than write your own, and end with `Resolves \
                 #<issue>`.\n"
            ),
            Template::Folder(path) => format!(
                "The repo has no pull request templates of its own, so its owner's apply, in \
                 `{path}` of `{repo}`. Each is in `{file}`, by name: read it before you write \
                 the body, fill in the one that fits rather than write your own, and end with \
                 `Resolves #<issue>`.\n"
            ),
        }
    }

    /// The template's own text, for the worker to read from its build folder
    ///
    /// One `<template>` block, or one per file of a folder, each by name, cut
    /// at [`KEPT`].
    pub(super) fn text(&self) -> String {
        let blocks = match &self.found {
            Template::File(_) => self
                .texts
                .iter()
                .map(|(_, text)| format!("<template>\n{}</template>\n", closed(text)))
                .collect::<String>(),
            Template::Folder(_) => self
                .texts
                .iter()
                .map(|(name, text)| {
                    format!("<template name=\"{name}\">\n{}</template>\n", closed(text))
                })
                .collect::<String>(),
        };
        fitted(blocks)
    }
}

// The text with a last newline, so the tag after it starts a line
fn closed(text: &str) -> std::borrow::Cow<'_, str> {
    if text.is_empty() || text.ends_with('\n') {
        text.into()
    } else {
        format!("{text}\n").into()
    }
}

// `texts` cut at the end of the last line that fits in `KEPT`, with a line saying so
fn fitted(texts: String) -> String {
    if texts.len() <= KEPT {
        return texts;
    }
    let mut end = KEPT;
    while !texts.is_char_boundary(end) {
        end -= 1;
    }
    let kept = &texts[..end];
    let kept = kept.rfind('\n').map_or("", |at| &kept[..=at]);
    format!(
        "{kept}[cut here: the template is longer than {} KiB]\n",
        KEPT / 1024
    )
}

/// The template in `owner`'s `.github` repository, if it has one
///
/// Looks where [`super::instructions`] looks in a worktree, for the same names,
/// in a public repository only.
///
/// # Errors
///
/// [`ForgeError`] when the forge cannot be asked or answers what cannot be read.
/// A repo that is not there, or has no template, is `None`.
pub(super) fn read(forge: &dyn Forge, owner: &str) -> Result<Option<OwnerTemplate>, ForgeError> {
    let Ok(repo) = ForgeSlug::try_from(format!("{owner}/.github")) else {
        return Ok(None);
    };
    // A root that is not there means no repo, so no place in it is either.
    let Some(root) = forge.folder(&repo, "")? else {
        return Ok(None);
    };
    // GitHub applies the defaults of a public `.github` repository only.
    if forge.visibility(&repo)? != Visibility::Public {
        return Ok(None);
    }
    for place in PLACES {
        let entries = if place.is_empty() {
            Some(root.clone())
        } else {
            forge.folder(&repo, place)?
        };
        let Some(entries) = entries else {
            continue;
        };
        let named = entries.iter().map(|e| (e.name.as_str(), e.is_dir));
        let Some(found) = pick(place, named) else {
            continue;
        };
        let texts = match &found {
            Template::File(path) => vec![(path.clone(), forge.file_text(&repo, path)?)],
            Template::Folder(path) => {
                let mut files: Vec<String> = (forge.folder(&repo, path)?.unwrap_or_default())
                    .into_iter()
                    .filter(|e| !e.is_dir)
                    .map(|e| e.name)
                    .collect();
                files.sort();
                let mut texts = Vec::new();
                for name in files {
                    let text = forge.file_text(&repo, &format!("{path}/{name}"))?;
                    texts.push((name, text));
                }
                texts
            }
        };
        return Ok(Some(OwnerTemplate { repo, found, texts }));
    }
    Ok(None)
}

/// The owner's template as of now, where `last` is what the runner had
///
/// A failed read keeps `last`, and comes back as a line for the log.
pub(super) fn refreshed(
    forge: &dyn Forge,
    owner: &str,
    last: Option<OwnerTemplate>,
) -> (Option<OwnerTemplate>, Option<String>) {
    match read(forge, owner) {
        Ok(found) => (found, None),
        Err(e) => {
            let kept = if last.is_some() {
                "so the last one read stays"
            } else {
                "so the repo has none from its owner"
            };
            let line =
                format!("cannot read the pull request template of {owner}'s .github: {e}, {kept}");
            (last, Some(line))
        }
    }
}

/// Where a pull request template would come from for a repo checked out at
/// `checkout` and owned by `owner`, reading the owner's from the forge
///
/// # Errors
///
/// [`ForgeError`] when the repo has no template and the owner's cannot be read.
pub fn source_of(
    checkout: &Path,
    forge: &dyn Forge,
    owner: &str,
) -> Result<TemplateSource, ForgeError> {
    if template(checkout).is_some() {
        return Ok(TemplateSource::Repo);
    }
    Ok(read(forge, owner)?.map_or(TemplateSource::None, |t| {
        TemplateSource::Owner(t.repo().as_str().to_owned())
    }))
}

impl Runner {
    // Where a worker's template comes from, the repo's checkout before the owner's
    pub(super) fn template_source(&self) -> TemplateSource {
        if template(&self.settings.git.checkout).is_some() {
            return TemplateSource::Repo;
        }
        match &self.owner_template {
            Some(owners) => TemplateSource::Owner(owners.repo().as_str().to_owned()),
            None => TemplateSource::None,
        }
    }
}
