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
/// and only after the archive has taken the moved part. A replacement that
/// fails puts the archive back at the length it had, so the moved part is
/// never in it twice.
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
    let before = std::fs::metadata(archive).map_or(0, |m| m.len());
    append(archive, moved).map_err(|e| failed(archive, &e))?;
    if let Err(e) = replace(notes, kept) {
        // The moved part is in the archive and still in the notes, so the
        // next roll would write it into the archive a second time.
        return match truncate(archive, before) {
            Ok(()) => Err(failed(notes, &e)),
            Err(undo) => Err(format!("{}; {}", failed(notes, &e), failed(archive, &undo))),
        };
    }
    Ok(Some(moved.len()))
}

// The byte a roll cuts at, which keeps at least [`NOTES_KEEP`] and no more
// than [`NOTES_MAX`] in the notes: the latest `## ` entry's start that does,
// else the latest line's start that does, else the byte that does.
fn cut_at(text: &str) -> usize {
    let least = text.len().saturating_sub(NOTES_MAX);
    let most = text.len().saturating_sub(NOTES_KEEP);
    let mut line = 0;
    let mut entry = 0;
    for at in line_starts(text) {
        if at > most {
            break;
        }
        if at < least {
            continue;
        }
        line = at;
        if text[at..].starts_with("## ") {
            entry = at;
        }
    }
    match entry {
        at if at > 0 => at,
        _ if line > 0 => line,
        _ => boundary(text, most),
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

// Puts `file` back at `len` bytes
fn truncate(file: &Path, len: u64) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .append(true)
        .open(file)
        .and_then(|file| file.set_len(len))
}

fn failed(file: &Path, e: &std::io::Error) -> String {
    format!("{}: {}", file.display(), e.kind())
}

// The archive is there and holds something, without reading it whole
fn archive_holds(archive: &Path) -> bool {
    std::fs::metadata(archive).is_ok_and(|m| m.len() > 0)
}

impl Runner {
    /// Rolls the project manager's notes past their limit, and says what this
    /// wake must tell it of the three notes files
    pub(super) fn pm_notes(&mut self) -> prompt::Notes {
        let folder = self.paths.pm.clone();
        let standing = std::fs::read_to_string(folder.join(PM_STANDING)).unwrap_or_default();
        let condense = standing.len() > STANDING_MAX;
        let archive = folder.join(PM_ARCHIVE);
        // The archive is what a roll creates, so its absence is the first roll.
        // The standing file is no proof of one: an answered tell creates it.
        let first = !archive.exists();
        let rolled = match roll(&folder.join(PM_NOTES), &archive) {
            Ok(moved) => moved.is_some(),
            Err(why) => {
                self.notes.push(format!("cannot roll the notes: {why}"));
                false
            }
        };
        if rolled {
            self.archive_unmined();
            if first {
                self.first_roll(&folder);
            }
        }
        let mine = self.state.pm_unmined || (archive_holds(&archive) && standing.trim().is_empty());
        prompt::Notes {
            standing,
            rolled,
            mine,
            condense,
        }
    }

    // The archive now holds notes no `standing` answer has read. Saved with
    // the roll, so a wake that rolled and then failed, and a runner that
    // restarts after one, both still owe the ask.
    fn archive_unmined(&mut self) {
        self.state.pm_unmined = true;
        if let Err(e) = self.save(self.state.clone()) {
            self.notes.push(format!("cannot record the roll: {e}"));
        }
    }

    // The first roll: nothing has been moved out of the log before, so this
    // wake is told in full and mines the standing decisions out of the
    // archive. What a tell already put in the standing notes stays there.
    fn first_roll(&mut self, folder: &Path) {
        self.pm.full_prompt = true;
        let standing = folder.join(PM_STANDING);
        if standing.exists() {
            return;
        }
        if let Err(e) = std::fs::write(&standing, "") {
            self.notes
                .push(format!("cannot write {PM_STANDING}: {}", e.kind()));
        }
    }

    /// Writes `text` as the whole of the standing notes, or says why they
    /// stay as they are
    ///
    /// # Errors
    ///
    /// A message naming why the answer's `standing` was dropped, for its
    /// `dropped`.
    pub(super) fn pm_stand(&mut self, text: &str) -> Result<String, String> {
        // A tell is added after this, so the file always ends a line
        let body = match text.is_empty() {
            true => String::new(),
            false => format!("{}\n", text.trim_end()),
        };
        if body.len() > STANDING_MAX {
            return Err(format!(
                "`standing`: over its {} KiB limit, so {PM_STANDING} stays as it is",
                STANDING_MAX / 1024
            ));
        }
        replace(&self.paths.pm.join(PM_STANDING), &body)
            .map_err(|e| failed(Path::new(PM_STANDING), &e))?;
        // This answer is what the archive's older notes were read into
        self.state.pm_unmined = false;
        Ok(format!("set the standing notes, {} bytes", body.len()))
    }

    /// Adds each tell the wake carried to the end of the standing notes,
    /// dated, so a rule the maintainer gave cannot roll out of the log. An
    /// answer that gave the whole file was told to carry its own tells, so
    /// its caller does not add them a second time.
    /// Adds each tell the wake carried to the end of the standing notes,
    /// dated, so a rule the maintainer gave cannot roll out of the log. An
    /// answer that gave the whole file was told to carry its own tells, so
    /// its caller does not add them a second time.
    pub(super) fn pm_stand_tells(&mut self, told: &[String]) {
        if told.is_empty() {
            return;
        }
        let at = crate::board::briefing::date_time(self.ports.clock.now());
        let added: String = told
            .iter()
            .map(|tell| format!("## {at}\n{tell}\n"))
            .collect();
        let file = self.paths.pm.join(PM_STANDING);
        // A `standing` answer or a hand-written file can end mid-line, and a
        // dated tell must never start glued onto what is already there
        let gap = match std::fs::read_to_string(&file) {
            Ok(text) if !text.is_empty() && !text.ends_with('\n') => "\n",
            _ => "",
        };
        if let Err(e) = append(&file, &format!("{gap}{added}")) {
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

    #[test]
    fn a_roll_leaves_no_more_than_the_limit_in_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let (notes, archive) = files(dir.path());
        // A 9,993-byte entry and then a 40,008-byte one: cutting at the second
        // entry's start would leave 40 KiB in the log, over what the prompt and
        // the docs promise, so the cut goes inside it, at a line's start.
        let text = format!(
            "## short\n{}## long\n{}",
            line().repeat(156),
            line().repeat(625)
        );
        std::fs::write(&notes, &text).unwrap();

        assert!(roll(&notes, &archive).unwrap().is_some());
        let kept = read(&notes);
        assert!(kept.len() <= NOTES_MAX, "{} bytes kept", kept.len());
        assert!(kept.len() >= NOTES_KEEP);
        assert_eq!(format!("{}{kept}", read(&archive)), text);
        assert!(!kept.contains("## "), "both entries rolled away");
    }

    #[test]
    fn a_roll_that_cannot_write_the_notes_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let (notes, archive) = files(dir.path());
        let text = entries(40);
        std::fs::write(&notes, &text).unwrap();
        // A folder where the temporary file belongs stops the notes being replaced
        std::fs::create_dir(dir.path().join("pm-notes.md.tmp")).unwrap();

        let why = roll(&notes, &archive).expect_err("the notes cannot be replaced");
        assert!(why.contains("pm-notes.md"), "{why}");
        assert_eq!(read(&notes), text, "the log is as it was");
        assert!(
            read(&archive).is_empty(),
            "the archive is put back as it was"
        );

        std::fs::remove_dir_all(dir.path().join("pm-notes.md.tmp")).unwrap();
        let moved = roll(&notes, &archive)
            .unwrap()
            .expect("the roll runs again");
        let kept = read(&notes);
        assert_eq!(moved, text.len() - kept.len());
        assert_eq!(
            format!("{}{kept}", read(&archive)),
            text,
            "no part of the log is in the archive twice"
        );
    }
}
