//! Signalling child processes.

use rustix::process::{Pid, Signal, kill_process};

/// Ask process `pid` to terminate gracefully (SIGTERM). Returns `false` if
/// the process no longer exists.
pub fn terminate(pid: u32) -> bool {
    let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
        return false;
    };
    kill_process(pid, Signal::TERM).is_ok()
}
