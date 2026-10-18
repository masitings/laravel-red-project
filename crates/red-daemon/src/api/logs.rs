//! `/v1/logs`: RED's own logs, service and PHP-FPM logs, and site logs
//! such as Laravel's `storage/logs/*.log`.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use red_api::{ApiError, ErrorCode, LogEntry, LogFile, LogKind, LogResponse, endpoints};
use serde::Deserialize;

use super::{ApiResponse, ApiResult, App};
use crate::AppState;

/// At most this much of the end of a file is read.
const MAX_READ: u64 = 4 * 1024 * 1024;
/// Entries returned when the request does not say.
const DEFAULT_LINES: usize = 500;
const MAX_LINES: usize = 5000;
/// Log directories inside a project, relative to its root.
const SITE_LOG_DIRS: [&str; 3] = ["storage/logs", "var/log", "logs"];

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(endpoints::LOGS, get(list))
        .route(endpoints::LOG_READ, get(read))
}

fn file(id: String, kind: LogKind, label: String, path: &Path) -> Option<LogFile> {
    let meta = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or_default();
    Some(LogFile {
        id,
        kind,
        label,
        path: path.display().to_string(),
        size: meta.len(),
        modified,
    })
}

fn log_files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "log") && p.is_file())
        .collect();
    files.sort();
    files
}

/// Every log RED knows about. Ids are recomputed on every request, so a
/// client can only ever read files from this list.
fn all(app: &AppState) -> Vec<LogFile> {
    let paths = &app.paths;
    let mut logs = Vec::new();
    logs.extend(file(
        "daemon".into(),
        LogKind::Daemon,
        "redd".into(),
        &paths.daemon_log(),
    ));
    for install in app.php.installs() {
        let v = &install.version;
        logs.extend(file(
            format!("php:{v}"),
            LogKind::Php,
            format!("PHP {v} (FPM)"),
            &paths.log_dir.join(format!("php{v}-fpm.log")),
        ));
    }
    for name in app.state_snapshot().services.keys() {
        logs.extend(file(
            format!("service:{name}"),
            LogKind::Service,
            name.clone(),
            &paths.service_log(name),
        ));
    }
    for site in red_core::sites::list(&app.state_snapshot()) {
        for dir in SITE_LOG_DIRS {
            for path in log_files_in(&site.path.join(dir)) {
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                logs.extend(file(
                    format!("site:{}:{dir}/{file_name}", site.name),
                    LogKind::Site,
                    format!("{} · {file_name}", site.name),
                    &path,
                ));
            }
        }
    }
    logs
}

async fn list(State(app): App) -> Json<Vec<LogFile>> {
    Json(all(&app))
}

#[derive(Debug, Deserialize)]
struct ReadQuery {
    id: String,
    lines: Option<usize>,
}

async fn read(State(app): App, Query(query): Query<ReadQuery>) -> ApiResult<LogResponse> {
    let file = all(&app)
        .into_iter()
        .find(|f| f.id == query.id)
        .ok_or_else(|| {
            ApiResponse::from(
                ApiError::new(ErrorCode::NotFound, format!("no log {:?}", query.id))
                    .with_hint("list logs with `red logs`"),
            )
        })?;
    let limit = query.lines.unwrap_or(DEFAULT_LINES).clamp(1, MAX_LINES);
    let path = PathBuf::from(&file.path);
    let (text, cut) = tokio::task::spawn_blocking(move || tail(&path))
        .await
        .map_err(super::internal)?
        .map_err(super::internal)?;
    let mut entries: Vec<LogEntry> = red_core::logs::parse(&text)
        .into_iter()
        .map(|e| LogEntry {
            time: e.time,
            level: e.level,
            channel: e.channel,
            message: e.message,
        })
        .collect();
    let truncated = cut || entries.len() > limit;
    if entries.len() > limit {
        entries.drain(..entries.len() - limit);
    }
    Ok(Json(LogResponse {
        file,
        entries,
        truncated,
    }))
}

/// The end of a file as text, starting at a line boundary. The flag is
/// `true` when the beginning of the file was skipped.
fn tail(path: &Path) -> std::io::Result<(String, bool)> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(MAX_READ);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.read_to_end(&mut bytes)?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0
        && let Some(newline) = text.find('\n')
    {
        text.drain(..=newline);
    }
    Ok((text, start > 0))
}
