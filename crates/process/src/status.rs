//! How a process ended, as the number a shell gives

use std::process::ExitStatus;

/// What `status` is as a shell reports it: the code the process exited with, or
/// 128 plus the signal that ended it
///
/// `None` when it has neither. Windows has no signals, so a process there
/// always has a code.
#[must_use]
pub fn shell_number(status: ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.code().or_else(|| status.signal().map(|s| 128 + s))
    }
    #[cfg(not(unix))]
    {
        status.code()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_code_is_itself_and_a_signal_is_128_more() {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(shell_number(ExitStatus::from_raw(3 << 8)), Some(3));
        assert_eq!(shell_number(ExitStatus::from_raw(15)), Some(143));
    }

    #[cfg(windows)]
    #[test]
    fn a_code_is_itself() {
        use std::os::windows::process::ExitStatusExt;
        assert_eq!(shell_number(ExitStatus::from_raw(3)), Some(3));
        assert_eq!(
            shell_number(ExitStatus::from_raw(0xC000_013A)),
            Some(-1_073_741_510)
        );
    }
}
