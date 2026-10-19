//! Commands the webview may invoke.

use red_api::client::{Client, ClientError, Method};
use red_platform::Paths;
use serde::Serialize;

/// Client for the local redd, or the reason there is none.
#[derive(Debug)]
pub struct ApiState(Result<Client, String>);

impl ApiState {
    pub fn discover() -> Self {
        Self(
            Paths::discover()
                .map(|paths| Client::new(paths.socket()))
                .map_err(|err| err.to_string()),
        )
    }

    #[cfg(test)]
    pub fn with_client(client: Client) -> Self {
        Self(Ok(client))
    }
}

/// Error shape sent to the webview. Matches `RedApiError` in
/// `src/api/client.ts`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CommandError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

impl CommandError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            hint: None,
            status: None,
        }
    }
}

impl From<ClientError> for CommandError {
    fn from(err: ClientError) -> Self {
        match err {
            ClientError::DaemonNotRunning { .. } => Self {
                hint: Some("Start the daemon with `redd`.".to_owned()),
                ..Self::new("daemon_not_running", err.to_string())
            },
            ClientError::Api { status, error } => Self {
                code: serde_json::to_value(error.code)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "internal".to_owned()),
                message: error.message,
                hint: error.hint,
                status: Some(status),
            },
            other => Self::new("internal", other.to_string()),
        }
    }
}

/// Forward one Control API request to redd and return its JSON body.
#[tauri::command]
pub async fn api(
    state: tauri::State<'_, ApiState>,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, CommandError> {
    forward(&state, &method, &path, body).await
}

async fn forward(
    state: &ApiState,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, CommandError> {
    let client = state
        .0
        .as_ref()
        .map_err(|err| CommandError::new("internal", err.clone()))?;
    if !path.starts_with("/v") {
        return Err(CommandError::new(
            "bad_request",
            format!("not a Control API path: {path}"),
        ));
    }
    let method = Method::from_bytes(method.as_bytes())
        .map_err(|_| CommandError::new("bad_request", format!("invalid method {method}")))?;
    Ok(client.request(method, path, body).await?)
}

/// Download a raw message or an attachment from the mail catcher and
/// write it to `destination` (chosen by the user in a save dialog).
#[tauri::command]
pub async fn save_mail_file(
    state: tauri::State<'_, ApiState>,
    path: String,
    destination: String,
) -> Result<(), CommandError> {
    save(&state, &path, &destination).await
}

async fn save(state: &ApiState, path: &str, destination: &str) -> Result<(), CommandError> {
    let client = state
        .0
        .as_ref()
        .map_err(|err| CommandError::new("internal", err.clone()))?;
    if !path.starts_with("/v1/mail/") || path.contains("..") {
        return Err(CommandError::new(
            "bad_request",
            format!("not a mail download: {path}"),
        ));
    }
    let bytes = client.bytes(path).await?;
    std::fs::write(destination, &bytes)
        .map_err(|err| CommandError::new("internal", format!("cannot write {destination}: {err}")))
}

/// Start redd in the background: the copy bundled with the app, else one
/// installed on this machine. Returns the binary that was started.
#[tauri::command]
pub fn start_daemon() -> Result<String, CommandError> {
    let redd = find_redd().ok_or_else(|| CommandError {
        hint: Some(
            "Install RED's command-line tools with `just install`, then run `red setup`.".into(),
        ),
        ..CommandError::new("not_found", "redd was not found")
    })?;
    let paths = Paths::discover().map_err(|err| CommandError::new("internal", err.to_string()))?;
    std::fs::create_dir_all(&paths.log_dir)
        .map_err(|err| CommandError::new("internal", err.to_string()))?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.daemon_log())
        .map_err(|err| CommandError::new("internal", err.to_string()))?;
    let err_log = log
        .try_clone()
        .map_err(|err| CommandError::new("internal", err.to_string()))?;
    std::process::Command::new(&redd)
        .stdin(std::process::Stdio::null())
        .stdout(log)
        .stderr(err_log)
        .spawn()
        .map_err(|err| {
            CommandError::new(
                "internal",
                format!("cannot start {}: {err}", redd.display()),
            )
        })?;
    Ok(redd.display().to_string())
}

/// Restart redd through its login agent (`red restart`), e.g. after the app
/// updated itself and the running daemon is still the old version.
#[tauri::command]
pub async fn restart_daemon() -> Result<String, CommandError> {
    let red = bundled("red")
        .into_iter()
        .chain(home_bin("red"))
        .chain(red_platform::binaries::in_path("red"))
        .find(|p| p.is_file())
        .ok_or_else(|| CommandError {
            hint: Some("Install the command-line tool from the Status tab first.".into()),
            ..CommandError::new("not_found", "red was not found")
        })?;
    let output = tokio::process::Command::new(&red)
        .arg("restart")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|err| {
            CommandError::new("internal", format!("cannot run {}: {err}", red.display()))
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(CommandError::new(
            "internal",
            format!(
                "`red restart` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ))
    }
}

/// Whether this build can update itself (it was released with an update
/// signing key).
#[tauri::command]
pub fn updater_enabled(app: tauri::AppHandle) -> bool {
    crate::updater_configured(app.config())
}

/// Where the command-line tool is bundled and whether it is on `PATH`.
#[derive(Debug, Serialize)]
pub struct CliStatus {
    /// `red` inside the app bundle, if this is a packaged build.
    pub bundled: Option<String>,
    /// The `red` a shell would run, if any.
    pub installed: Option<String>,
}

#[tauri::command]
pub fn cli_status() -> CliStatus {
    CliStatus {
        bundled: bundled("red").map(|p| p.display().to_string()),
        installed: red_platform::binaries::in_path("red")
            .into_iter()
            .chain(home_bin("red"))
            .find(|p| p.is_file())
            .map(|p| p.display().to_string()),
    }
}

/// Put the bundled `red` on `PATH` in `~/.local/bin`: a symlink into the app
/// bundle, or, for an AppImage (whose files move on every launch), copies
/// of red, redd and red-helper that the app refreshes when it starts.
#[tauri::command]
pub fn install_cli() -> Result<String, CommandError> {
    let source = bundled("red").ok_or_else(|| {
        CommandError::new(
            "not_found",
            "this build does not include the command-line tool",
        )
    })?;
    let target = home_bin("red")
        .ok_or_else(|| CommandError::new("internal", "cannot find the home directory"))?;
    let bin_dir = target
        .parent()
        .ok_or_else(|| CommandError::new("internal", "bad target"))?
        .to_path_buf();
    std::fs::create_dir_all(&bin_dir)
        .map_err(|err| CommandError::new("internal", err.to_string()))?;
    let appimage = std::env::var_os("APPIMAGE").is_some();
    match std::fs::symlink_metadata(&target) {
        Ok(meta) if meta.file_type().is_symlink() => {
            std::fs::remove_file(&target)
                .map_err(|err| CommandError::new("internal", err.to_string()))?;
        }
        Ok(_) if appimage && bin_dir.join(APPIMAGE_MARKER).exists() => {}
        Ok(_) => {
            return Err(CommandError {
                hint: Some("Remove it first if you want the app's copy instead.".into()),
                ..CommandError::new("conflict", format!("{} already exists", target.display()))
            });
        }
        Err(_) => {}
    }
    if appimage {
        let dir = source
            .parent()
            .ok_or_else(|| CommandError::new("internal", "bad source"))?;
        copy_binaries(dir, &bin_dir)
            .map_err(|err| CommandError::new("internal", err.to_string()))?;
    } else {
        std::os::unix::fs::symlink(&source, &target)
            .map_err(|err| CommandError::new("internal", err.to_string()))?;
    }
    Ok(target.display().to_string())
}

use red_platform::services::APPIMAGE_MARKER;

const BINARIES: [&str; 3] = ["red", "redd", "red-helper"];

/// Copy RED's binaries from `from` into `to`, replacing each through a
/// rename so a running redd keeps its old file, and leave the marker.
fn copy_binaries(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    for name in BINARIES {
        let source = from.join(name);
        if !source.is_file() {
            continue;
        }
        let temp = to.join(format!(".{name}.new"));
        std::fs::copy(&source, &temp)?;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&temp, to.join(name))?;
    }
    std::fs::write(
        to.join(APPIMAGE_MARKER),
        "Copied by the RED AppImage; refreshed when it starts. `red uninstall` removes these.\n",
    )
}

/// When running as an AppImage that installed the command before, refresh
/// the copies so they match this (possibly updated) version.
pub fn refresh_appimage_cli() {
    if std::env::var_os("APPIMAGE").is_none() {
        return;
    }
    let (Some(source), Some(target)) = (bundled("red"), home_bin("red")) else {
        return;
    };
    let (Some(from), Some(to)) = (source.parent(), target.parent()) else {
        return;
    };
    if to.join(APPIMAGE_MARKER).exists() {
        let _ = copy_binaries(from, to);
    }
}

fn bundled(name: &str) -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|p| p.is_file())
}

fn home_bin(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".local/bin").join(name))
}

/// redd next to the app's executable (bundled), in `~/.local/bin`, or on `PATH`.
fn find_redd() -> Option<std::path::PathBuf> {
    bundled("redd")
        .into_iter()
        .chain(home_bin("redd"))
        .chain(red_platform::binaries::in_path("redd"))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_daemon_becomes_daemon_not_running() {
        let home = tempfile::tempdir().unwrap();
        let state = ApiState::with_client(Client::new(home.path().join("redd.sock")));
        let err = forward(&state, "GET", "/v1/status", None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "daemon_not_running");
        assert!(err.hint.is_some());
    }

    #[tokio::test]
    async fn only_mail_files_can_be_saved() {
        let home = tempfile::tempdir().unwrap();
        let state = ApiState::with_client(Client::new(home.path().join("redd.sock")));
        let dest = home.path().join("out").display().to_string();
        let err = save(&state, "/v1/status", &dest).await.unwrap_err();
        assert_eq!(err.code, "bad_request");
        let err = save(&state, "/v1/mail/../status", &dest).await.unwrap_err();
        assert_eq!(err.code, "bad_request");
    }

    #[test]
    fn copies_binaries_with_a_marker() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        for name in BINARIES {
            std::fs::write(from.path().join(name), name).unwrap();
        }
        std::fs::write(to.path().join("redd"), "old").unwrap();
        copy_binaries(from.path(), to.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(to.path().join("redd")).unwrap(),
            "redd"
        );
        assert!(to.path().join(APPIMAGE_MARKER).is_file());
        assert!(!to.path().join(".red.new").exists());
    }

    #[tokio::test]
    async fn rejects_non_api_paths_and_bad_methods() {
        let home = tempfile::tempdir().unwrap();
        let state = ApiState::with_client(Client::new(home.path().join("redd.sock")));
        let err = forward(&state, "GET", "http://evil", None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "bad_request");
        let err = forward(&state, "G ET", "/v1/status", None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "bad_request");
    }
}
