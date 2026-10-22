//! A stand-in script that is safe to run at once
//!
//! Uses std alone, so a test in any crate can use it.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Writes an executable stand-in script that is safe to run at once.
///
/// On Linux a child forked while the file is still open for writing holds
/// it, and `exec` of it fails with ETXTBSY. Other tests fork all the time,
/// so this waits until the script has been executed once, with the probe
/// variable set, before handing it over. The script's first line after the
/// shebang exits on the probe, so the probe run does nothing.
///
/// Windows runs no shebang. There the script is written as it is, and runs only
/// where Git for Windows' `sh` is the one starting it, as a hook does. A stand-in
/// that a test starts itself is a [`write_fake`](super::write_fake).
pub fn write_script(path: &Path, contents: &str) {
    use std::io::Write;

    let (shebang, rest) = contents.split_once('\n').expect("a script has a shebang");
    assert!(shebang.starts_with("#!"), "{shebang:?}");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    executable(&mut options);
    let mut file = options.open(path).unwrap();
    write!(
        file,
        "{shebang}\n[ -n \"$KELPIE_TEST_PROBE\" ] && exit 0\n{rest}"
    )
    .unwrap();
    drop(file);
    if cfg!(windows) {
        return;
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match Command::new(path).env("KELPIE_TEST_PROBE", "1").status() {
            Ok(_) => return,
            Err(e) if e.raw_os_error() == Some(26) && std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("cannot run the stand-in script {}: {e}", path.display()),
        }
    }
}

#[cfg(unix)]
fn executable(options: &mut std::fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;

    options.mode(0o755);
}

#[cfg(not(unix))]
fn executable(_: &mut std::fs::OpenOptions) {}

/// Makes an owner-only FIFO at `path`
///
/// Runs `mkfifo(1)`: `rustix` has no `mkfifoat` on macOS, and `nix` would put
/// a second copy of itself in the tree beside the one `shep-core` builds.
///
/// # Panics
///
/// On Windows, which has no FIFO file. A test that needs one is
/// `#[cfg_attr(windows, ignore = "...")]`.
#[track_caller]
pub fn make_fifo(path: &Path) {
    #[cfg(windows)]
    {
        let _ = path;
        panic!("Windows has no FIFO file");
    }
    #[cfg(not(windows))]
    {
        let made = Command::new("mkfifo")
            .args(["-m", "600"])
            .arg(path)
            .status()
            .unwrap();
        assert!(made.success(), "mkfifo {}: {made}", path.display());
    }
}

/// Shell text that waits for `marker` to exist
///
/// Ends when the marker exists, when the script's parent (`$PPID`) is gone,
/// or after `within`, so a stub left running by a test that failed or was
/// killed ends by itself. The bound counts 20 ms sleeps and so runs a little
/// over `within`. Put it where the script would loop on the marker.
pub fn waits_for(marker: &Path, within: Duration) -> String {
    const NAP_MS: u128 = 20;
    let naps = within.as_millis().div_ceil(NAP_MS);
    let marker = marker.display().to_string().replace('\'', "'\\''");
    format!(
        "naps=0; while [ ! -e '{marker}' ] && kill -0 $PPID 2>/dev/null && [ $naps -lt {naps} ]; \
         do sleep 0.02; naps=$((naps + 1)); done"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::short_tempdir;
    use std::process::Child;
    use std::time::Instant;

    const ENDS_BY: Duration = Duration::from_secs(5);

    fn waiting_stub(dir: &Path, marker: &Path, within: Duration) -> std::path::PathBuf {
        let stub = dir.join("stub");
        let body = format!(
            "#!/bin/sh\n{}\ntouch {}\n",
            waits_for(marker, within),
            dir.join("ended").display()
        );
        write_script(&stub, &body);
        stub
    }

    fn ends_within(child: &mut Child, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    #[cfg_attr(windows, ignore = "the stub is a shell script")]
    fn a_waiting_stub_ends_once_its_marker_exists() {
        let dir = short_tempdir();
        let marker = dir.path().join("go");
        let stub = waiting_stub(dir.path(), &marker, Duration::from_secs(60));
        let mut child = Command::new(&stub).spawn().unwrap();

        std::thread::sleep(Duration::from_millis(200));
        assert!(child.try_wait().unwrap().is_none());
        std::fs::write(&marker, "").unwrap();

        assert!(ends_within(&mut child, ENDS_BY));
        assert!(dir.path().join("ended").exists());
    }

    #[test]
    #[cfg_attr(windows, ignore = "the stub is a shell script")]
    fn a_waiting_stub_ends_when_its_parent_is_gone() {
        let dir = short_tempdir();
        let marker = dir.path().join("go");
        let stub = waiting_stub(dir.path(), &marker, Duration::from_secs(60));

        let parent = Command::new("sh")
            .arg("-c")
            .arg(format!("{} &", stub.display()))
            .status()
            .unwrap();
        assert!(parent.success());

        let deadline = Instant::now() + ENDS_BY;
        while !dir.path().join("ended").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(dir.path().join("ended").exists());
    }

    #[test]
    #[cfg_attr(windows, ignore = "the stub is a shell script")]
    fn a_waiting_stub_ends_when_its_bound_passes_with_no_marker() {
        let dir = short_tempdir();
        let marker = dir.path().join("go");
        let stub = waiting_stub(dir.path(), &marker, Duration::from_millis(300));
        let mut child = Command::new(&stub).spawn().unwrap();

        assert!(ends_within(&mut child, ENDS_BY));
        assert!(dir.path().join("ended").exists());
        assert!(!marker.exists());
    }
}
