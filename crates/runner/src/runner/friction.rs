//! The `Friction` section of a pull request's body
//!
//! A worker may end its pull request's body with a `Friction` section: a
//! line or two on what slowed it down, written while its session's cache is
//! warm. Kelpie keeps it in the journal when the work item finishes, for the
//! batch retro to read.

use std::path::Path;

use super::{Runner, pull_request_file};
use crate::work_item::WorkItem;

/// The most of a section kept, in characters
const LONGEST: usize = 600;

/// What a section cut short ends with
const CUT: &str = " (cut)";

/// The line a harness puts at the end of a body, which is no friction
fn trailer(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("\u{1f916} Generated with") || line.starts_with("Generated with")
}

/// The text under the `Friction` heading of `body`, to the next heading or
/// the harness's own trailer
///
/// `None` with no such heading, or nothing under it.
pub(super) fn section(body: &str) -> Option<String> {
    let heading = |line: &str| line.trim_start().starts_with('#');
    let named = |line: &str| {
        heading(line)
            && line
                .trim_start_matches(|c: char| c == '#' || c.is_whitespace())
                .trim_end_matches(':')
                .trim()
                .eq_ignore_ascii_case("friction")
    };
    let mut lines = body.lines().skip_while(|line| !named(line)).skip(1);
    let closes = |line: &&str| {
        let line = line.trim_start().to_ascii_lowercase();
        line.starts_with("resolves #") || line.starts_with("closes #")
    };
    let text: Vec<&str> = (lines.by_ref())
        .take_while(|line| !heading(line) && !trailer(line))
        .filter(|line| !closes(line))
        .collect();
    let text = text.join("\n");
    let text = text.trim();
    (!text.is_empty()).then(|| kept(text))
}

// `text` kept to [`LONGEST`], ending at a sentence where the window holds
// one, at a word where it holds none, and marked when anything went
// unkept
fn kept(text: &str) -> String {
    let all: Vec<char> = text.chars().collect();
    if all.len() <= LONGEST {
        return text.to_owned();
    }
    let window = &all[..LONGEST];
    let cut = (window.iter())
        .rposition(|c| matches!(c, '.' | '!' | '?'))
        .or_else(|| window.iter().rposition(|c| c.is_whitespace()))
        .unwrap_or(LONGEST - 1);
    let kept: String = window[..=cut].iter().collect();
    format!("{}{CUT}", kept.trim_end())
}

impl Runner {
    /// The friction `item`'s worker reported in its pull request's body, from
    /// the file it handed over or else the forge's copy
    pub(super) fn friction_of(&self, item: &WorkItem) -> Option<String> {
        let file = crate::worker_folder::read(&item.build, Path::new(pull_request_file::FILE));
        let handed = match file {
            Ok(Some(file)) => pull_request_file::parse(&file).ok().map(|text| text.body),
            _ => None,
        };
        let body = match handed {
            Some(body) => body,
            None => {
                let number = item.pull_request?;
                self.ports.forge.reviewed(&self.remote, number).ok()?.body
            }
        };
        section(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::section;

    #[test]
    fn the_section_runs_to_the_next_heading() {
        let body = "## What this changes\n\n- a\n\n## Friction\n\nThe lockfile fought me.\n\
                    Twice.\n\n## Before you ask\n\n- [ ] done\n\nResolves #7";
        assert_eq!(
            section(body).as_deref(),
            Some("The lockfile fought me.\nTwice.")
        );
    }

    #[test]
    fn the_closing_line_is_not_friction() {
        let body = "## Friction\n\nSlow build.\n\nResolves #7\n";
        assert_eq!(section(body).as_deref(), Some("Slow build."));
    }

    #[test]
    fn the_section_may_end_the_body() {
        let body = "Resolves #7\n\n### friction:\nSlow build.\n";
        assert_eq!(section(body).as_deref(), Some("Slow build."));
    }

    #[test]
    fn no_heading_or_an_empty_one_is_no_friction() {
        assert_eq!(section("Resolves #7\n"), None);
        assert_eq!(section("## Friction\n\n## Next\n"), None);
        assert_eq!(section("A friction in prose, not a heading.\n"), None);
    }

    #[test]
    fn the_generated_with_trailer_ends_the_section() {
        let body = "## Friction\n\nSlow build.\n\n\u{1f916} Generated with \
                    [Claude Code](https://claude.com/claude-code)\n";
        assert_eq!(section(body).as_deref(), Some("Slow build."));
    }

    #[test]
    fn a_cut_section_ends_at_its_last_sentence_and_says_it_is_cut() {
        let said = "The lockfile fought me.".repeat(40);
        let kept = section(&format!("## Friction\n\n{said}\nAnd a tail.\n")).unwrap();
        assert!(kept.ends_with(". (cut)"), "{kept}");
        assert!(kept.chars().count() <= 607, "{kept}");
        assert!(!kept.contains("tail"), "{kept}");
    }

    #[test]
    fn a_cut_section_with_no_sentence_end_ends_at_a_whole_word() {
        let said = "word ".repeat(200);
        let kept = section(&format!("## Friction\n\n{said}\n")).unwrap();
        assert!(kept.ends_with("word (cut)"), "{kept}");
        assert!(kept.chars().count() <= 607, "{kept}");
    }

    #[test]
    fn a_section_that_fits_is_kept_whole_and_unmarked() {
        let said = "x".repeat(600);
        assert_eq!(
            section(&format!("## Friction\n\n{said}\n")).as_deref(),
            Some(&*said)
        );
    }
}
