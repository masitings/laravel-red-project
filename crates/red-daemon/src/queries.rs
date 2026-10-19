//! Laravel's database queries, live, without a package in the project.
//!
//! While query capture is on, PHP-FPM pools start with
//! `auto_prepend_file` set to a small script RED writes. For Laravel apps
//! it adds RED's service provider through Laravel's own package manifest
//! (a copy kept in RED's data directory, chosen with `APP_PACKAGES_CACHE`,
//! so the project's files are not touched). The provider listens for
//! `QueryExecuted` and sends one JSON line per query to the dump server,
//! which hands lines starting with `{` to [`Queries::receive`].

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use red_api::QueryEntry;
use serde::Deserialize;

/// Queries kept in memory.
pub const KEEP_QUERIES: usize = 1000;

/// Captured queries.
#[derive(Debug)]
pub struct Queries {
    next_id: AtomicU64,
    entries: Mutex<VecDeque<QueryEntry>>,
    dir: PathBuf,
}

/// One line the provider sends.
#[derive(Debug, Deserialize)]
struct Line {
    red: String,
    sql: String,
    #[serde(default)]
    bindings: Vec<String>,
    #[serde(default)]
    time: f64,
    #[serde(default)]
    connection: Option<String>,
    #[serde(default)]
    request: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    at: Option<f64>,
}

impl Queries {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            next_id: AtomicU64::new(1),
            entries: Mutex::new(VecDeque::new()),
            dir: data_dir.join("inspector"),
        }
    }

    /// Captured queries, oldest first.
    pub fn list(&self) -> Vec<QueryEntry> {
        self.entries
            .lock()
            .expect("queries poisoned")
            .iter()
            .cloned()
            .collect()
    }

    pub fn clear(&self) {
        self.entries.lock().expect("queries poisoned").clear();
    }

    /// Take one JSON line from the provider; `false` when it is not one.
    pub fn receive(&self, line: &[u8]) -> bool {
        let Ok(line) = serde_json::from_slice::<Line>(line) else {
            return false;
        };
        if line.red != "query" {
            return false;
        }
        let entry = QueryEntry {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            received: line.at.map_or_else(now_ms, |t| (t * 1000.0) as u64),
            bound: bind(&line.sql, &line.bindings),
            sql: line.sql,
            bindings: line.bindings,
            time_ms: line.time,
            connection: line.connection,
            request: line.request,
            request_id: line.request_id,
            file: line.file,
            line: line.line,
        };
        let mut entries = self.entries.lock().expect("queries poisoned");
        entries.push_back(entry);
        while entries.len() > KEEP_QUERIES {
            entries.pop_front();
        }
        true
    }

    /// The prepend script, written (or refreshed) for `dump_port`.
    pub fn prepend_file(&self, dump_port: u16) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(self.dir.join("cache"))?;
        let provider = self.dir.join("provider.php");
        write_if_changed(&provider, PROVIDER)?;
        let prepend = self.dir.join("prepend.php");
        let script = PREPEND
            .replace("{{PORT}}", &dump_port.to_string())
            .replace("{{CACHE}}", &php_string(&self.dir.join("cache")))
            .replace("{{PROVIDER}}", &php_string(&provider));
        write_if_changed(&prepend, &script)?;
        Ok(prepend)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn write_if_changed(path: &Path, contents: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).ok().as_deref() != Some(contents) {
        std::fs::write(path, contents)?;
    }
    Ok(())
}

/// A single-quoted PHP string literal.
fn php_string(path: &Path) -> String {
    format!(
        "'{}'",
        path.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    )
}

/// The query with its bindings filled in, for reading and copying. Values
/// arrive already quoted by the provider (strings) or bare (numbers, NULL).
pub fn bind(sql: &str, bindings: &[String]) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut values = bindings.iter();
    let mut quote: Option<char> = None;
    for c in sql.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                out.push(c);
            }
            (Some(_), c) => out.push(c),
            (None, '\'' | '"' | '`') => {
                quote = Some(c);
                out.push(c);
            }
            (None, '?') => match values.next() {
                Some(value) => out.push_str(value),
                None => out.push('?'),
            },
            (None, c) => out.push(c),
        }
    }
    out
}

/// Runs before every PHP request while capture is on. Only touches Laravel
/// apps, and only through environment variables Laravel reads.
const PREPEND: &str = r#"<?php
// Generated by RED Project for its query inspector. Safe to delete; RED
// writes it again while query capture is on.
(static function (): void {
    $script = $_SERVER['SCRIPT_FILENAME'] ?? '';
    if ($script === '') {
        return;
    }
    $root = dirname($script, 2);
    if (!is_file($root . '/artisan') || !is_dir($root . '/vendor/laravel/framework')) {
        return;
    }
    $manifest = $root . '/bootstrap/cache/packages.php';
    if (!is_file($manifest)) {
        return; // Laravel writes it on its first request; RED joins in on the next.
    }
    $cache = {{CACHE}};
    $key = md5($root);
    $packages = $cache . '/' . $key . '-packages.php';
    if (!is_file($packages) || filemtime($packages) < filemtime($manifest) || filemtime($packages) < filemtime(__FILE__)) {
        $list = require $manifest;
        if (!is_array($list)) {
            return;
        }
        $list['red-project/inspector'] = ['providers' => ['RedProject\\Inspector\\Provider']];
        $tmp = $packages . '.' . getmypid();
        if (@file_put_contents($tmp, '<?php return ' . var_export($list, true) . ';') === false || !@rename($tmp, $packages)) {
            return;
        }
    }
    $services = $cache . '/' . $key . '-services.php';
    foreach (['APP_PACKAGES_CACHE' => $packages, 'APP_SERVICES_CACHE' => $services] as $name => $value) {
        $_SERVER[$name] = $_ENV[$name] = $value;
        putenv($name . '=' . $value);
    }
    define('RED_INSPECTOR_PORT', {{PORT}});
    spl_autoload_register(static function (string $class): void {
        if ($class === 'RedProject\\Inspector\\Provider') {
            require {{PROVIDER}};
        }
    });
})();
"#;

/// Laravel service provider that reports queries to RED.
const PROVIDER: &str = r#"<?php
// Generated by RED Project: sends Laravel's queries to RED's inspector.

namespace RedProject\Inspector;

final class Provider extends \Illuminate\Support\ServiceProvider
{
    /** @var resource|null|false */
    private static $socket = null;
    private static string $requestId = '';

    public function boot(): void
    {
        self::$requestId = bin2hex(random_bytes(6));
        $this->app['events']->listen(
            \Illuminate\Database\Events\QueryExecuted::class,
            static function ($event): void {
                self::send($event);
            }
        );
    }

    private static function send($event): void
    {
        if (self::$socket === false) {
            return;
        }
        if (self::$socket === null) {
            $port = \defined('RED_INSPECTOR_PORT') ? RED_INSPECTOR_PORT : 9912;
            self::$socket = @stream_socket_client("tcp://127.0.0.1:{$port}", $errno, $error, 0.2);
            if (self::$socket === false) {
                return;
            }
        }
        $bindings = [];
        foreach ((array) $event->bindings as $value) {
            $bindings[] = self::literal($value);
        }
        [$file, $line] = self::caller();
        $method = $_SERVER['REQUEST_METHOD'] ?? 'CLI';
        $uri = $_SERVER['REQUEST_URI'] ?? '';
        $json = json_encode([
            'red' => 'query',
            'sql' => (string) $event->sql,
            'bindings' => $bindings,
            'time' => (float) $event->time,
            'connection' => (string) $event->connectionName,
            'request' => trim($method . ' ' . $uri),
            'request_id' => self::$requestId,
            'file' => $file,
            'line' => $line,
            'at' => microtime(true),
        ], JSON_INVALID_UTF8_SUBSTITUTE | JSON_PARTIAL_OUTPUT_ON_ERROR);
        if ($json !== false && @fwrite(self::$socket, $json . "\n") === false) {
            self::$socket = false;
        }
    }

    private static function literal($value): string
    {
        if ($value === null) {
            return 'NULL';
        }
        if (\is_bool($value)) {
            return $value ? '1' : '0';
        }
        if (\is_int($value) || \is_float($value)) {
            return (string) $value;
        }
        if ($value instanceof \DateTimeInterface) {
            $value = $value->format('Y-m-d H:i:s');
        }
        if (\is_object($value) && !method_exists($value, '__toString')) {
            return "'[" . \get_class($value) . "]'";
        }
        $text = (string) $value;
        if (!mb_check_encoding($text, 'UTF-8')) {
            return "'[binary " . \strlen($text) . " bytes]'";
        }
        if (\strlen($text) > 500) {
            $text = substr($text, 0, 500) . '…';
        }
        return "'" . str_replace("'", "''", $text) . "'";
    }

    /** The first frame in the app itself, not in vendor/. */
    private static function caller(): array
    {
        foreach (debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 60) as $frame) {
            $file = $frame['file'] ?? '';
            if ($file !== '' && !str_contains($file, '/vendor/') && $file !== __FILE__) {
                return [$file, $frame['line'] ?? null];
            }
        }
        return [null, null];
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_in_bindings_outside_quotes() {
        assert_eq!(
            bind(
                "select * from users where id = ? and name = ?",
                &["5".into(), "'Ann'".into()]
            ),
            "select * from users where id = 5 and name = 'Ann'"
        );
        assert_eq!(
            bind("select '?' as q, `a?` from t where x = ?", &["1".into()]),
            "select '?' as q, `a?` from t where x = 1"
        );
        assert_eq!(
            bind("where a = ? and b = ?", &["1".into()]),
            "where a = 1 and b = ?"
        );
    }

    #[test]
    fn receives_query_lines() {
        let dir = tempfile::tempdir().unwrap();
        let queries = Queries::new(dir.path());
        assert!(queries.receive(
            br#"{"red":"query","sql":"select * from users where id = ?","bindings":["3"],"time":1.25,"connection":"mysql","request":"GET /users","request_id":"ab12","file":"/app/Http/Controllers/UserController.php","line":12,"at":1792000000.5}"#
        ));
        assert!(!queries.receive(b"{\"red\":\"other\",\"sql\":\"x\"}"));
        assert!(!queries.receive(b"not json"));
        let list = queries.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].bound, "select * from users where id = 3");
        assert_eq!(list[0].received, 1_792_000_000_500);
        assert_eq!(list[0].time_ms, 1.25);
        assert_eq!(list[0].line, Some(12));
        queries.clear();
        assert!(queries.list().is_empty());
    }

    #[test]
    fn writes_the_hook_files() {
        let dir = tempfile::tempdir().unwrap();
        let queries = Queries::new(dir.path());
        let prepend = queries.prepend_file(9912).unwrap();
        let text = std::fs::read_to_string(&prepend).unwrap();
        assert!(text.contains("define('RED_INSPECTOR_PORT', 9912);"));
        assert!(!text.contains("{{"));
        assert!(dir.path().join("inspector/provider.php").is_file());
        assert!(dir.path().join("inspector/cache").is_dir());
    }
}
