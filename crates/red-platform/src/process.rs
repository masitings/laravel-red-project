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

/// Ask process `pid` to shut down quickly (SIGINT). PostgreSQL treats this
/// as a "fast" shutdown, while SIGTERM waits for clients to disconnect.
pub fn interrupt(pid: u32) -> bool {
    let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
        return false;
    };
    kill_process(pid, Signal::INT).is_ok()
}

/// Name of the user running the current process, from `USER`/`LOGNAME`.
pub fn current_user_name() -> Option<String> {
    ["USER", "LOGNAME"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()))
}
