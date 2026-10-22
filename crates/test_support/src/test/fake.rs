//! A stand-in command that runs on Windows too
//!
//! A [`Fake`] says what the command does in a few steps. [`write_fake`] writes
//! it as an `sh` script on macOS and Linux and as a batch file on Windows,
//! where a file runs only if its name ends in `.cmd`, so the caller runs the
//! path it gets back. A command that needs more than these steps stays a
//! [`write_script`](super::write_script), which Windows runs only through
//! Git for Windows' `sh`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::write_script;

#[derive(Debug)]
enum Step {
    Print(String),
    PrintErr(String),
    Touch(PathBuf),
    RecordArgs(PathBuf),
    PrintFile(PathBuf),
    Sleep(u32),
    Exit(i32),
}

/// What a stand-in command does, step by step
#[derive(Debug, Default)]
pub struct Fake(Vec<Step>);

impl Fake {
    /// A command that does nothing and exits 0
    pub fn new() -> Self {
        Self::default()
    }

    /// Prints `line` to stdout
    #[must_use]
    pub fn prints(mut self, line: &str) -> Self {
        self.0.push(Step::Print(line.to_owned()));
        self
    }

    /// Prints `line` to stderr
    #[must_use]
    pub fn prints_to_stderr(mut self, line: &str) -> Self {
        self.0.push(Step::PrintErr(line.to_owned()));
        self
    }

    /// Makes an empty file at `path`
    #[must_use]
    pub fn touches(mut self, path: &Path) -> Self {
        self.0.push(Step::Touch(path.to_owned()));
        self
    }

    /// Writes its arguments to `path`, one per line
    #[must_use]
    pub fn records_args(mut self, path: &Path) -> Self {
        self.0.push(Step::RecordArgs(path.to_owned()));
        self
    }

    /// Prints the file at `path` to stdout
    #[must_use]
    pub fn prints_file(mut self, path: &Path) -> Self {
        self.0.push(Step::PrintFile(path.to_owned()));
        self
    }

    /// Waits `seconds`
    #[must_use]
    pub fn sleeps(mut self, seconds: u32) -> Self {
        self.0.push(Step::Sleep(seconds));
        self
    }

    /// Ends with `code`, so the steps after it never run
    #[must_use]
    pub fn exits(mut self, code: i32) -> Self {
        self.0.push(Step::Exit(code));
        self
    }
}

/// Writes `fake` as `name` in `dir`, and returns the path to run
///
/// The path ends in `.cmd` on Windows.
pub fn write_fake(dir: &Path, name: &str, fake: &Fake) -> PathBuf {
    if cfg!(windows) {
        let path = dir.join(format!("{name}.cmd"));
        std::fs::write(&path, fake.batch()).unwrap();
        path
    } else {
        let path = dir.join(name);
        write_script(&path, &fake.shell());
        path
    }
}

fn sh_word(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

// A batch file's `echo` takes none of these as plain text.
fn batch_text(text: &str) -> &str {
    assert!(
        !text.contains(['%', '^', '&', '|', '<', '>', '!', '(', ')']),
        "a batch file cannot print {text:?} as it is"
    );
    text
}

impl Fake {
    // The steps, ending in the exit 0 that a fake with no `exits` gets.
    fn steps(&self) -> impl Iterator<Item = &Step> {
        const OK: Step = Step::Exit(0);
        self.0
            .iter()
            .chain(std::iter::once(&OK).filter(|_| !matches!(self.0.last(), Some(Step::Exit(_)))))
    }

    fn shell(&self) -> String {
        let mut script = String::from("#!/bin/sh\n");
        let steps: Vec<_> = self.steps().collect();
        for (at, step) in steps.iter().enumerate() {
            let line = match step {
                Step::Print(text) => format!("printf '%s\\n' {}", sh_word(text)),
                Step::PrintErr(text) => format!("printf '%s\\n' {} >&2", sh_word(text)),
                Step::Touch(path) => format!(": > {}", sh_word(&path.display().to_string())),
                Step::RecordArgs(path) => format!(
                    "for arg in \"$@\"; do printf '%s\\n' \"$arg\"; done > {}",
                    sh_word(&path.display().to_string())
                ),
                Step::PrintFile(path) => format!("cat {}", sh_word(&path.display().to_string())),
                // The shell becomes the sleep, so killing the fake by pid leaves nothing behind.
                Step::Sleep(seconds)
                    if steps[at + 1..].iter().all(|s| matches!(s, Step::Exit(0))) =>
                {
                    format!("exec sleep {seconds}")
                }
                Step::Sleep(seconds) => format!("sleep {seconds}"),
                Step::Exit(code) => format!("exit {code}"),
            };
            script.push_str(&line);
            script.push('\n');
        }
        script
    }

    fn batch(&self) -> String {
        let mut script = String::from("@echo off\r\n");
        for (at, step) in self.steps().enumerate() {
            match step {
                Step::Print(text) => writeln!(script, "echo {}\r", batch_text(text)),
                Step::PrintErr(text) => writeln!(script, ">&2 echo {}\r", batch_text(text)),
                Step::Touch(path) => writeln!(script, "type nul > \"{}\"\r", batch_path(path)),
                Step::RecordArgs(path) => {
                    let path = batch_path(path);
                    write!(
                        script,
                        "type nul > \"{path}\"\r\n:args{at}\r\nif \"%~1\"==\"\" goto done{at}\r\n\
                         >>\"{path}\" echo %~1\r\nshift\r\ngoto args{at}\r\n:done{at}\r\n"
                    )
                }
                Step::PrintFile(path) => writeln!(script, "type \"{}\"\r", batch_path(path)),
                // `ping` counts one second between its tries.
                Step::Sleep(seconds) => {
                    writeln!(script, "ping -n {} 127.0.0.1 >nul\r", seconds + 1)
                }
                Step::Exit(code) => writeln!(script, "exit /b {code}\r"),
            }
            .unwrap();
        }
        script
    }
}

// A path goes in double quotes, so it holds none.
fn batch_path(path: &Path) -> &str {
    let text = batch_text(path.to_str().expect("a path of text"));
    assert!(!text.contains('"'), "{text:?}");
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::short_tempdir;
    use std::process::Command;

    #[test]
    fn a_fake_prints_to_both_streams_and_exits_with_its_code() {
        let dir = short_tempdir();
        let fake = Fake::new()
            .prints("it's out")
            .prints_to_stderr("it's err")
            .exits(3);
        let program = write_fake(dir.path(), "fake", &fake);

        let out = Command::new(program).output().unwrap();

        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "it's out");
        assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "it's err");
        assert_eq!(out.status.code(), Some(3));
    }

    #[test]
    fn a_fake_records_its_arguments_and_makes_its_marker_and_prints_a_file() {
        let dir = short_tempdir();
        let seen = dir.path().join("seen");
        let ran = dir.path().join("ran");
        let fake = Fake::new()
            .touches(&ran)
            .records_args(&seen)
            .prints_file(&seen);
        let program = write_fake(dir.path(), "fake", &fake);

        let out = Command::new(program).args(["one", "two"]).output().unwrap();

        assert!(ran.exists());
        let lines = |text: &str| text.lines().map(str::to_owned).collect::<Vec<_>>();
        let seen = std::fs::read_to_string(seen).unwrap();
        assert_eq!(lines(&seen), ["one", "two"]);
        assert_eq!(lines(&String::from_utf8_lossy(&out.stdout)), ["one", "two"]);
    }

    #[test]
    fn a_fake_with_no_steps_exits_0() {
        let dir = short_tempdir();
        let program = write_fake(dir.path(), "fake", &Fake::new());

        assert_eq!(Command::new(program).status().unwrap().code(), Some(0));
    }

    #[test]
    fn a_fake_that_sleeps_ends_after_the_wait() {
        let dir = short_tempdir();
        let program = write_fake(dir.path(), "fake", &Fake::new().sleeps(1).exits(4));
        let started = std::time::Instant::now();

        let status = Command::new(program).status().unwrap();

        assert_eq!(status.code(), Some(4));
        assert!(started.elapsed() >= std::time::Duration::from_millis(900));
    }

    #[cfg(unix)]
    #[test]
    fn a_fake_that_ends_asleep_leaves_nothing_behind_when_killed() {
        use std::io::Read;
        let dir = short_tempdir();
        let program = write_fake(dir.path(), "fake", &Fake::new().sleeps(30));
        let mut child = Command::new(program)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = child.stdout.take().unwrap();
        let started = std::time::Instant::now();

        child.kill().unwrap();
        child.wait().unwrap();
        out.read_to_end(&mut Vec::new()).unwrap();

        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    #[test]
    fn a_fake_is_a_batch_file_on_windows() {
        let fake = Fake::new()
            .prints("hello")
            .prints_to_stderr("bad")
            .touches(Path::new("C:/t/ran"))
            .records_args(Path::new("C:/t/seen"))
            .sleeps(2)
            .exits(3);
        assert_eq!(
            fake.batch(),
            "@echo off\r\necho hello\r\n>&2 echo bad\r\ntype nul > \"C:/t/ran\"\r\n\
             type nul > \"C:/t/seen\"\r\n:args3\r\nif \"%~1\"==\"\" goto done3\r\n\
             >>\"C:/t/seen\" echo %~1\r\nshift\r\ngoto args3\r\n:done3\r\n\
             ping -n 3 127.0.0.1 >nul\r\nexit /b 3\r\n"
        );
    }
}
