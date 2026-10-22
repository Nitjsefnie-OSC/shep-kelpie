use std::process::ExitStatus;

/// The status of a process that exited with `code`
#[must_use]
pub fn exited(code: i32) -> ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(u32::try_from(code).expect("a test's exit code is not negative"))
    }
}

/// The status of a process that signal `signal` ended
#[cfg(unix)]
#[must_use]
pub fn signalled(signal: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(signal)
}
