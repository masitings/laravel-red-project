//! Information about the user running the current process.

/// Real user id of the current process.
pub fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// `true` when the current process runs as root.
pub fn is_root() -> bool {
    current_uid() == 0
}
