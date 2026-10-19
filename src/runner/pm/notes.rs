//! The project manager's three notes files: the log it appends to, the
//! archive kelpie rolls it into, and the standing notes only kelpie writes
//!
//! `pm-notes.md` stays the project manager's own file, which the `confine
//! --append` hook holds it to. Past [`NOTES_MAX`] a wake moves its older part
//! to the end of `pm-archive.md`, which the project manager greps and never
//! reads whole. `pm-standing.md` holds what a later wake must keep, written
//! only by kelpie: the project manager's `standing` answer, and each `tell`
//! it answered, so a rule the maintainer gave cannot roll away with the log.

use std::io::Write as _;
use std::path::Path;

use crate::ports::PM_NOTES;
use crate::runner::Runner;

use super::prompt;

/// The file the rolled part of the notes moves to, at its end
pub(super) const PM_ARCHIVE: &str = "pm-archive.md";

/// The file of what a later wake must keep, which only kelpie writes
pub(super) const PM_STANDING: &str = "pm-standing.md";

/// `pm-notes.md` past this many bytes is rolled at the next wake
///
/// Measured on this project: 428 KiB of notes, 4,493 lines, is about 100k
/// tokens, so 32 KiB is about 8k tokens read on every wake that reads them.
pub(super) const NOTES_MAX: usize = 32 * 1024;

/// The newest part a roll keeps in `pm-notes.md`, in bytes, about 4k tokens
pub(super) const NOTES_KEEP: usize = 16 * 1024;

/// The most `pm-standing.md` holds, in bytes, about 1k tokens
pub(super) const STANDING_MAX: usize = 4 * 1024;

/// Moves the older part of `notes` to the end of `archive`, and says how many
/// bytes it moved
///
/// A file at or under [`NOTES_MAX`] is left alone, and so is one that is not
/// there. The notes are written through a temporary file in the same folder,
/// and only after the archive has taken the moved part.
///
/// # Errors
///
/// A message naming the file that could not be read or written.
pub(super) fn roll(notes: &Path, archive: &Path) -> Result<Option<usize>, String> {
    let text = match std::fs::read_to_string(notes) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(failed(notes, &e)),
    };
    if text.len() <= NOTES_MAX {
        return Ok(None);
    }
    let at = cut_at(&text);
    let (moved, kept) = text.split_at(at);
    append(archive, moved).map_err(|e| failed(archive, &e))?;
    replace(notes, kept).map_err(|e| failed(notes, &e))?;
    Ok(Some(moved.len()))
}

// The byte a roll cuts at: the latest `## ` entry's start that leaves at
// least [`NOTES_KEEP`] in the notes, else the latest line's start that does,
// else the byte that leaves that much.
fn cut_at(text: &str) -> usize {
    let last = text.len().saturating_sub(NOTES_KEEP);
    let mut line = 0;
    let mut entry = 0;
    for at in line_starts(text) {
        if at > last {
            break;
        }
        line = at;
        if text[at..].starts_with("## ") {
            entry = at;
        }
    }
    match entry {
        at if at > 0 => at,
        _ if line > 0 => line,
        _ => boundary(text, last),
    }
}

// The byte at or below `at` that starts a character
fn boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn line_starts(text: &str) -> impl Iterator<Item = usize> {
    std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1))
}

// Writes `text` over `file` through a temporary file beside it, so the
// project manager never sees a half-written log.
fn replace(file: &Path, text: &str) -> std::io::Result<()> {
    let temp = file.with_extension("md.tmp");
    std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, file))
}

fn append(file: &Path, text: &str) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .and_then(|mut file| file.write_all(text.as_bytes()))
}

fn failed(file: &Path, e: &std::io::Error) -> String {
    format!("{}: {}", file.display(), e.kind())
}

impl Runner {
    /// Rolls the project manager's notes past their limit, and says what this
    /// wake must tell it of the three notes files
    pub(super) fn pm_notes(&mut self) -> prompt::Notes {
        let folder = self.paths.pm.clone();
        let standing = std::fs::read_to_string(folder.join(PM_STANDING)).unwrap_or_default();
        let condense = standing.len() > STANDING_MAX;
        let rolled = match roll(&folder.join(PM_NOTES), &folder.join(PM_ARCHIVE)) {
            Ok(moved) => moved.is_some(),
            Err(why) => {
                self.notes.push(format!("cannot roll the notes: {why}"));
                false
            }
        };
        if rolled && !folder.join(PM_STANDING).exists() {
            self.first_roll(&folder);
        }
        prompt::Notes {
            standing,
            rolled,
            condense,
        }
    }

    // The first roll: nothing has been kept out of the log yet, so this wake
    // is told in full and mines the standing decisions out of the archive.
    fn first_roll(&mut self, folder: &Path) {
        if let Err(e) = std::fs::write(folder.join(PM_STANDING), "") {
            self.notes
                .push(format!("cannot write {PM_STANDING}: {}", e.kind()));
        }
        self.pm.full_prompt = true;
    }

    /// Writes `text` as the whole of the standing notes, or says why they
    /// stay as they are
    ///
    /// # Errors
    ///
    /// A message naming why the answer's `standing` was dropped, for its
    /// `dropped`.
    pub(super) fn pm_stand(&mut self, text: &str) -> Result<String, String> {
        if text.len() > STANDING_MAX {
            return Err(format!(
                "`standing`: over its {} KiB limit, so {PM_STANDING} stays as it is",
                STANDING_MAX / 1024
            ));
        }
        replace(&self.paths.pm.join(PM_STANDING), text)
            .map_err(|e| failed(Path::new(PM_STANDING), &e))?;
        Ok(format!("set the standing notes, {} bytes", text.len()))
    }

    /// Adds each tell the wake carried to the end of the standing notes,
    /// dated, so a rule the maintainer gave cannot roll out of the log
    pub(super) fn pm_stand_tells(&mut self, told: &[String]) {
        if told.is_empty() {
            return;
        }
        let at = crate::board::briefing::date_time(self.ports.clock.now());
        let added: String = told
            .iter()
            .map(|tell| format!("## {at}\n{tell}\n"))
            .collect();
        if let Err(e) = append(&self.paths.pm.join(PM_STANDING), &added) {
            self.notes
                .push(format!("cannot add to {PM_STANDING}: {}", e.kind()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A log of `n` entries, each a `## wake <n>` heading over 20 lines of 64
    /// bytes, so entry 1 is 1,290 bytes and entry 10 up are 1,291
    fn entries(n: usize) -> String {
        (1..=n)
            .map(|i| format!("## wake {i}\n{}", line().repeat(20)))
            .collect()
    }

    fn line() -> String {
        "x".repeat(63) + "\n"
    }

    fn files(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        (dir.join(PM_NOTES), dir.join(PM_ARCHIVE))
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn a_file_under_the_limit_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (notes, archive) = files(dir.path());
        let text = entries(9);
        std::fs::write(&notes, &text).unwrap();

        assert_eq!(roll(&notes, &archive).unwrap(), None);
        assert_eq!(read(&notes), text);
        assert!(
            !archive.exists(),
            "a roll that moved nothing wrote no archive"
        );
    }

    #[test]
    fn a_roll_cuts_at_an_entry_and_the_archive_plus_the_notes_hold_every_byte_once() {
        let dir = tempfile::tempdir().unwrap();
        let (notes, archive) = files(dir.path());
        let text = entries(40);
        std::fs::write(&notes, &text).unwrap();

        let moved = roll(&notes, &archive)
            .unwrap()
            .expect("a 51,631-byte log rolls");
        let kept = read(&notes);
        let rolled = read(&archive);
        assert_eq!(moved, text.len() - kept.len());
        assert_eq!(format!("{rolled}{kept}"), text, "every byte held once");
        // Worked out by hand: the latest `## ` start that leaves 16 KiB or
        // more of a 51,631-byte log is entry 28's, at 34,848.
        assert!(kept.starts_with("## wake 28\n"), "{}", &kept[..40]);
        assert!(kept.len() >= NOTES_KEEP);
        assert!(rolled.ends_with(&line()));
    }

    #[test]
    fn one_entry_over_the_limit_and_a_file_with_no_heading_still_roll() {
        let dir = tempfile::tempdir().unwrap();
        let (notes, archive) = files(dir.path());
        let one = format!("## one entry\n{}", line().repeat(600));
        std::fs::write(&notes, &one).unwrap();
        assert!(roll(&notes, &archive).unwrap().is_some());
        let kept = read(&notes);
        assert_eq!(format!("{}{kept}", read(&archive)), one);
        assert!(kept.len() >= NOTES_KEEP);
        assert!(!kept.contains("## "), "the one entry rolled away");

        let plain = format!("- picked #12\n{}", line().repeat(600));
        std::fs::write(&notes, &plain).unwrap();
        std::fs::remove_file(&archive).unwrap();
        assert!(roll(&notes, &archive).unwrap().is_some());
        let kept = read(&notes);
        assert_eq!(format!("{}{kept}", read(&archive)), plain);
        assert!(kept.len() >= NOTES_KEEP);
        assert!(kept.starts_with(&line()));

        let whole = "y".repeat(40 * 1024);
        std::fs::write(&notes, &whole).unwrap();
        std::fs::remove_file(&archive).unwrap();
        assert!(roll(&notes, &archive).unwrap().is_some());
        let kept = read(&notes);
        assert_eq!(format!("{}{kept}", read(&archive)), whole);
        assert_eq!(
            kept.len(),
            NOTES_KEEP,
            "a log with no line end is cut at one"
        );
    }

    #[test]
    fn a_second_roll_appends_to_the_archive() {
        let dir = tempfile::tempdir().unwrap();
        let (notes, archive) = files(dir.path());
        std::fs::write(&notes, entries(40)).unwrap();

        let first = roll(&notes, &archive).unwrap().expect("the first roll");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&notes)
            .and_then(|mut file| file.write_all(entries(20).as_bytes()))
            .unwrap();
        let second = roll(&notes, &archive).unwrap().expect("the second roll");

        let all = format!("{}{}", entries(40), entries(20));
        let kept = read(&notes);
        assert_eq!(read(&archive), &all[..all.len() - kept.len()]);
        assert_eq!(format!("{}{kept}", read(&archive)), all);
        assert_eq!(first + second, all.len() - kept.len());
        assert!(read(&archive).contains("## wake 40\n"));
        assert!(kept.starts_with("## wake "), "{kept}");
    }
}
