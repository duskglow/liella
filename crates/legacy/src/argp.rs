//! argp's historical semantics (spec §1.7 ⚑ HIST; D-1.7): when the C interface prints its
//! messages and when it ends the process. The parser itself (`liella::argp`) only returns
//! results and errors; these rules apply them to the flags of a `struct argp_state`.
//!
//! * `ARGP_NO_ERRS` in the state silences `argp_error`, `argp_failure`, `argp_usage` and
//!   `argp_state_help` completely, and so also prevents their exits.
//! * `ARGP_NO_EXIT` in the state prevents every exit; the functions then return. `--version`
//!   still exits under `ARGP_NO_ERRS` alone.
//! * Without a state, messages are printed and exits happen.

use liella::argp::{HELP_EXIT_ERR, HELP_EXIT_OK, NO_ERRS, NO_EXIT};

/// Whether a function given a state with `flags` (`None`: no state) prints anything at all.
pub fn reports(flags: Option<u32>) -> bool {
    !flags.is_some_and(|f| f & NO_ERRS != 0)
}

/// Whether a state with `flags` (`None`: no state) lets the process exit.
fn exits(flags: Option<u32>) -> bool {
    !flags.is_some_and(|f| f & (NO_EXIT | NO_ERRS) != 0)
}

/// The exit status after help printed with the `ARGP_HELP_*` `help` flags for a state with
/// `flags`: `err_status` (`argp_err_exit_status`) for `ARGP_HELP_EXIT_ERR`, 0 for
/// `ARGP_HELP_EXIT_OK`, `None` to return.
pub fn help_exit(flags: Option<u32>, help: u32, err_status: i32) -> Option<i32> {
    if !exits(flags) {
        return None;
    }
    if help & HELP_EXIT_ERR != 0 {
        Some(err_status)
    } else if help & HELP_EXIT_OK != 0 {
        Some(0)
    } else {
        None
    }
}

/// `argp_failure`'s exit: `status` when it is not 0 and the state allows it.
pub fn failure_exit(flags: Option<u32>, status: i32) -> Option<i32> {
    (status != 0 && exits(flags)).then_some(status)
}

/// The exit after `--version`: 0, unless `ARGP_NO_EXIT` prevents it (`ARGP_NO_ERRS` does not:
/// printing the version is no error).
pub fn version_exit(flags: u32) -> Option<i32> {
    (flags & NO_EXIT == 0).then_some(0)
}

/// Where the program name a parse reports starts in `argv[0]`: after its last `/`.
pub fn base_name(argv0: &[u8]) -> usize {
    argv0.iter().rposition(|&b| b == b'/').map_or(0, |i| i.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use liella::argp::{HELP_STD_ERR, HELP_STD_HELP};

    #[test]
    fn exits_follow_the_state_flags() {
        assert_eq!(help_exit(None, HELP_STD_ERR, 64), Some(64));
        assert_eq!(help_exit(Some(0), HELP_STD_HELP, 64), Some(0));
        assert_eq!(help_exit(Some(NO_EXIT), HELP_STD_ERR, 64), None);
        assert_eq!(help_exit(Some(0), 0, 64), None);
        assert_eq!(failure_exit(Some(0), 3), Some(3));
        assert_eq!(failure_exit(Some(NO_ERRS), 3), None);
        assert_eq!(failure_exit(None, 0), None);
        assert_eq!(version_exit(NO_EXIT), None);
        assert_eq!(version_exit(NO_ERRS), Some(0));
        assert!(!reports(Some(NO_ERRS)) && reports(Some(NO_EXIT)) && reports(None));
        assert_eq!(base_name(b"/usr/bin/prog"), 9);
        assert_eq!(base_name(b"prog"), 0);
    }
}
