//! OS-specific glue for RED Project.
//!
//! Everything that differs between macOS and Linux lives in this crate so the
//! rest of the workspace can stay free of `cfg(target_os)` branches.

pub mod paths;
pub mod php;
pub mod process;
pub mod user;

pub use paths::Paths;

/// Short name of the operating system this build targets.
pub const fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unsupported"
    }
}
