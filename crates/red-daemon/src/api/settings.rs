//! `/v1/settings` (RED's config file) and `/v1/php/{version}/settings`
//! (php.ini settings per PHP version).

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path as UrlPath, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use red_api::{
    PhpListResponse, PhpSettingsRequest, PhpVersionInfo, SettingsResponse, SettingsUpdate,
    endpoints,
};
use red_core::config::{Config, set_key};

use super::{ApiResult, App, bad_request, installed_version, internal, list_php};
use crate::AppState;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(endpoints::SETTINGS, get(settings).put(update_settings))
        .route("/v1/php/{version}/settings", put(set_php_settings))
        .route("/v1/php/{version}/info", get(php_info))
}

fn response(app: &AppState, config: &Config) -> SettingsResponse {
    SettingsResponse {
        tld: config.tld.clone(),
        http_port: config.http_port,
        https_port: config.https_port,
        smtp_port: config.smtp_port,
        dump_port: config.dump_port,
        xdebug_port: config.xdebug_port,
        runtimes_auto_update: config.runtimes_auto_update,
        config_file: app.paths.config_file().display().to_string(),
        restart_required: *config != app.config,
    }
}

async fn settings(State(app): App) -> ApiResult<SettingsResponse> {
    let config = Config::load(&app.paths.config_file()).map_err(internal)?;
    Ok(Json(response(&app, &config)))
}

/// Write the changes into `config.toml`, keeping the rest of the file.
async fn update_settings(
    State(app): App,
    Json(body): Json<SettingsUpdate>,
) -> ApiResult<SettingsResponse> {
    let path = app.paths.config_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(internal(err)),
    };
    let current = Config::parse(&text).map_err(|e| bad_request(e.to_string()))?;
    let mut text = text;
    let ports = [
        ("smtp_port", body.smtp_port),
        ("dump_port", body.dump_port),
        ("xdebug_port", body.xdebug_port),
    ];
    for (key, port) in ports {
        if let Some(port) = port {
            if port < 1024 {
                return Err(bad_request(format!("{key} must be 1024 or higher")).into());
            }
            text = set_key(&text, key, &toml::Value::Integer(port.into()));
        }
    }
    if let Some(on) = body.runtimes_auto_update {
        text = set_key(&text, "runtimes_auto_update", &toml::Value::Boolean(on));
    }
    let next = Config::parse(&text).map_err(|e| bad_request(e.to_string()))?;
    let used = [
        next.http_port,
        next.https_port,
        next.dns_port,
        next.smtp_port,
        next.dump_port,
        next.xdebug_port,
    ];
    if (1..used.len()).any(|i| used[..i].contains(&used[i])) {
        return Err(bad_request("two of RED's ports would be the same".to_owned()).into());
    }
    if next != current {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(internal)?;
        }
        std::fs::write(&path, &text).map_err(internal)?;
        tracing::info!(path = %path.display(), "settings changed; they apply after a restart");
    }
    Ok(Json(response(&app, &next)))
}

/// Replace RED's php.ini settings for one version and restart its pools.
async fn set_php_settings(
    State(app): App,
    UrlPath(version): UrlPath<String>,
    Json(body): Json<PhpSettingsRequest>,
) -> ApiResult<PhpListResponse> {
    let version = installed_version(&app, &version).await?;
    let mut settings = BTreeMap::new();
    for (key, value) in body.settings {
        let (key, value) = (key.trim().to_owned(), value.trim().to_owned());
        if value.is_empty() {
            continue;
        }
        red_core::php_ini::validate(&key, &value).map_err(bad_request)?;
        settings.insert(key, value);
    }
    app.update_state(|s| {
        if settings.is_empty() {
            s.php_settings.remove(&version);
        } else {
            s.php_settings.insert(version.clone(), settings);
        }
    })
    .map_err(internal)?;
    app.php.restart(&version).await;
    app.notify(red_api::Event::PhpChanged);
    Ok(list_php(State(app)).await)
}

/// Ask the version's PHP CLI, with RED's settings, what it runs with.
async fn php_info(State(app): App, UrlPath(version): UrlPath<String>) -> ApiResult<PhpVersionInfo> {
    let version = installed_version(&app, &version).await?;
    let install = app
        .php
        .find(&version)
        .ok_or_else(|| internal(format!("PHP {version} disappeared")))?;
    let php = red_platform::binaries::php_cli_for_fpm(&install.fpm).ok_or_else(|| {
        bad_request(format!(
            "no PHP CLI found next to {}",
            install.fpm.display()
        ))
    })?;
    let settings = app
        .state_snapshot()
        .php_settings
        .get(&version)
        .cloned()
        .unwrap_or_default();
    let keys: Vec<&str> = red_core::php_ini::COMMON.iter().map(|(k, _)| *k).collect();
    let script = format!(
        "echo json_encode(['ini' => php_ini_loaded_file(), 'ext' => get_loaded_extensions(), \
         'values' => array_map(fn($k) => (string) ini_get($k), array_combine({0}, {0}))]);",
        serde_json::to_string(&keys).map_err(internal)?
    );
    let mut command = tokio::process::Command::new(&php);
    for arg in red_core::php_ini::args(&settings) {
        command.arg("-d").arg(arg);
    }
    let output = command
        .arg("-r")
        .arg(script)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(internal)?;
    if !output.status.success() {
        return Err(internal(format!(
            "{} failed: {}",
            php.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(Json(
        parse_info(&version, &output.stdout).map_err(internal)?,
    ))
}

fn parse_info(version: &str, stdout: &[u8]) -> Result<PhpVersionInfo, String> {
    #[derive(serde::Deserialize)]
    struct Raw {
        ini: serde_json::Value,
        ext: Vec<String>,
        values: BTreeMap<String, String>,
    }
    // Startup warnings may come before the JSON.
    let text = String::from_utf8_lossy(stdout);
    let json = text.find('{').map_or("", |at| &text[at..]);
    let raw: Raw = serde_json::from_str(json).map_err(|e| format!("unexpected PHP output: {e}"))?;
    let mut extensions: Vec<String> = raw.ext.into_iter().map(|e| e.to_lowercase()).collect();
    extensions.sort();
    extensions.dedup();
    Ok(PhpVersionInfo {
        version: version.to_owned(),
        ini_file: raw.ini.as_str().map(str::to_owned),
        extensions,
        values: raw.values,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_php_info() {
        let out = br#"PHP Warning: x
{"ini":false,"ext":["Core","pdo_mysql","json"],"values":{"memory_limit":"128M"}}"#;
        let info = parse_info("8.4", out).unwrap();
        assert_eq!(info.ini_file, None);
        assert_eq!(info.extensions, ["core", "json", "pdo_mysql"]);
        assert_eq!(info.values["memory_limit"], "128M");
        let info = parse_info("8.4", br#"{"ini":"/etc/php.ini","ext":[],"values":{}}"#).unwrap();
        assert_eq!(info.ini_file.as_deref(), Some("/etc/php.ini"));
    }
}
