//! Service definitions for the helper (root, launchd daemon / systemd
//! service) and for `redd` itself (per user, launchd agent / systemd user
//! service). This module only knows names, paths and file contents; the CLI
//! does the installing.

use std::path::{Path, PathBuf};

/// launchd label / systemd unit stem of the helper.
pub const HELPER_LABEL: &str = "dev.redproject.helper";
/// launchd label of the per-user daemon.
pub const AGENT_LABEL: &str = "dev.redproject.redd";

/// File the desktop AppImage leaves in `~/.local/bin` next to the copies of
/// red, redd and red-helper it made, so it can refresh them and
/// `red uninstall` can remove them.
pub const APPIMAGE_MARKER: &str = ".red-appimage";

/// Where the helper binary is installed.
pub fn helper_binary() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/PrivilegedHelperTools").join(HELPER_LABEL)
    } else {
        PathBuf::from("/usr/local/libexec/red-helper")
    }
}

/// Where the helper's service definition is installed.
pub fn helper_definition() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/LaunchDaemons").join(format!("{HELPER_LABEL}.plist"))
    } else {
        PathBuf::from("/etc/systemd/system/red-helper.service")
    }
}

/// Where the per-user daemon's service definition is installed.
pub fn agent_definition(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/LaunchAgents")
            .join(format!("{AGENT_LABEL}.plist"))
    } else {
        home.join(".config/systemd/user/redd.service")
    }
}

/// PATH given to redd (and through it to PHP), covering Homebrew on Apple
/// Silicon, Intel and Linux.
pub const AGENT_PATH: &str = "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/local/sbin:/usr/bin:/bin:/usr/sbin:/sbin:/home/linuxbrew/.linuxbrew/bin:/home/linuxbrew/.linuxbrew/sbin";

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn plist(label: &str, args: &[String], extra: &str) -> String {
    let args: String = args
        .iter()
        .map(|a| format!("        <string>{}</string>\n", xml_escape(a)))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <!-- Added by RED Project. -->\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20   <key>Label</key>\n\
         \x20   <string>{label}</string>\n\
         \x20   <key>ProgramArguments</key>\n\
         \x20   <array>\n\
         {args}\
         \x20   </array>\n\
         \x20   <key>RunAtLoad</key>\n\
         \x20   <true/>\n\
         \x20   <key>KeepAlive</key>\n\
         \x20   <true/>\n\
         {extra}\
         </dict>\n\
         </plist>\n"
    )
}

/// launchd daemon definition for the helper (macOS).
pub fn helper_plist(binary: &Path, socket: &Path, uid: u32) -> String {
    plist(
        HELPER_LABEL,
        &[
            binary.display().to_string(),
            "--socket".into(),
            socket.display().to_string(),
            "--allow-uid".into(),
            uid.to_string(),
        ],
        "    <key>StandardErrorPath</key>\n    <string>/var/log/red-helper.log</string>\n",
    )
}

/// launchd agent definition for redd (macOS).
pub fn agent_plist(redd: &Path, log: &Path) -> String {
    let log = xml_escape(&log.display().to_string());
    plist(
        AGENT_LABEL,
        &[redd.display().to_string()],
        &format!(
            "    <key>EnvironmentVariables</key>\n    <dict>\n        <key>PATH</key>\n        <string>{AGENT_PATH}</string>\n    </dict>\n\
             \x20   <key>StandardOutPath</key>\n    <string>{log}</string>\n\
             \x20   <key>StandardErrorPath</key>\n    <string>{log}</string>\n\
             \x20   <key>ProcessType</key>\n    <string>Interactive</string>\n"
        ),
    )
}

/// systemd unit for the helper (Linux).
pub fn helper_unit(binary: &Path, socket: &Path, uid: u32) -> String {
    format!(
        "# Added by RED Project.\n[Unit]\nDescription=RED Project privileged helper\n\n\
         [Service]\nExecStart={} --socket {} --allow-uid {uid}\nRestart=always\n\n\
         [Install]\nWantedBy=multi-user.target\n",
        binary.display(),
        socket.display()
    )
}

/// systemd user unit for redd (Linux).
pub fn agent_unit(redd: &Path) -> String {
    format!(
        "# Added by RED Project.\n[Unit]\nDescription=RED Project daemon\n\n\
         [Service]\nExecStart={}\nRestart=always\nEnvironment=PATH={AGENT_PATH}\n\n\
         [Install]\nWantedBy=default.target\n",
        redd.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_plist_runs_the_helper_for_one_user() {
        let text = helper_plist(
            Path::new("/Library/PrivilegedHelperTools/dev.redproject.helper"),
            Path::new("/var/run/red-helper.sock"),
            501,
        );
        assert!(text.contains("<string>dev.redproject.helper</string>"));
        assert!(text.contains("<string>--allow-uid</string>"));
        assert!(text.contains("<string>501</string>"));
        assert!(text.contains("<key>KeepAlive</key>"));
    }

    #[test]
    fn agent_plist_sets_path_and_log() {
        let text = agent_plist(
            Path::new("/Applications/RED.app/Contents/MacOS/redd"),
            Path::new("/Users/me/Library/Logs/RED/redd.log"),
        );
        assert!(text.contains("<string>/Applications/RED.app/Contents/MacOS/redd</string>"));
        assert!(text.contains("/opt/homebrew/bin"));
        assert!(text.contains("<string>/Users/me/Library/Logs/RED/redd.log</string>"));
    }

    #[test]
    fn escapes_xml() {
        let text = agent_plist(Path::new("/tmp/a&b/redd"), Path::new("/tmp/log"));
        assert!(text.contains("/tmp/a&amp;b/redd"));
    }

    #[test]
    fn units_name_binaries() {
        assert!(
            helper_unit(Path::new("/x/red-helper"), Path::new("/run/s"), 1000)
                .contains("ExecStart=/x/red-helper --socket /run/s --allow-uid 1000")
        );
        assert!(agent_unit(Path::new("/x/redd")).contains("ExecStart=/x/redd"));
    }
}
