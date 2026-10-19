//! Databases inside MySQL, MariaDB and PostgreSQL services: list, create,
//! drop, back up and restore, with the client tools that ship next to the
//! server (`mysql`/`mariadb`, `mysqldump`/`mariadb-dump`, `psql`,
//! `pg_dump`).
//!
//! Backups are gzipped SQL files in `<data>/backups/<service>/`; a restore
//! takes those or any other `.sql`/`.sql.gz` dump.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use red_api::{BackupInfo, DatabaseInfo};
use red_core::{ServiceConfig, ServiceKind};

/// Databases the server keeps for itself.
const SYSTEM: [&str; 7] = [
    "information_schema",
    "mysql",
    "performance_schema",
    "sys",
    "postgres",
    "template0",
    "template1",
];

/// `true` for a name RED creates or drops: letters, digits, `_` and `-`.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The client tools for one service.
#[derive(Debug, Clone)]
pub struct Tools {
    kind: ServiceKind,
    port: u16,
    client: PathBuf,
    dump: PathBuf,
}

impl Tools {
    pub fn for_service(config: &ServiceConfig) -> Result<Self, String> {
        let find = |names: &[&str], what: &str| {
            red_platform::binaries::companion(&config.binary, names)
                .ok_or_else(|| format!("{what} was not found next to {}", config.binary.display()))
        };
        let (client, dump) = match config.kind {
            ServiceKind::Mysql => (
                find(&["mariadb", "mysql"], "the mysql client")?,
                find(&["mariadb-dump", "mysqldump"], "mysqldump")?,
            ),
            ServiceKind::Postgres => (find(&["psql"], "psql")?, find(&["pg_dump"], "pg_dump")?),
            ServiceKind::Redis => return Err("Redis and Valkey have no databases to manage".into()),
        };
        Ok(Self {
            kind: config.kind,
            port: config.port,
            client,
            dump,
        })
    }

    fn command(&self, program: &Path) -> Command {
        let mut command = Command::new(program);
        let port = self.port.to_string();
        match self.kind {
            ServiceKind::Mysql => {
                command.args([
                    "--protocol=TCP",
                    "-h",
                    "127.0.0.1",
                    "-P",
                    &port,
                    "-u",
                    "root",
                ]);
            }
            _ => {
                command
                    .args(["-h", "127.0.0.1", "-p", &port, "-U", "root"])
                    .env("PGCONNECT_TIMEOUT", "10");
            }
        }
        command
    }

    /// Run one SQL statement; returns stdout.
    fn sql(&self, sql: &str, database: Option<&str>) -> Result<String, String> {
        let mut command = self.command(&self.client);
        match self.kind {
            ServiceKind::Mysql => {
                command.args(["-N", "-B", "-e", sql]);
                if let Some(db) = database {
                    command.arg(db);
                }
            }
            _ => {
                command.args(["-d", database.unwrap_or("postgres")]).args([
                    "-v",
                    "ON_ERROR_STOP=1",
                    "-At",
                    "-F",
                    "\t",
                    "-c",
                    sql,
                ]);
            }
        }
        run(command)
    }

    pub fn list(&self) -> Result<Vec<DatabaseInfo>, String> {
        let sql = match self.kind {
            ServiceKind::Mysql => {
                "SELECT s.schema_name, COALESCE(SUM(t.data_length + t.index_length), 0) \
                 FROM information_schema.schemata s LEFT JOIN information_schema.tables t \
                 ON t.table_schema = s.schema_name GROUP BY s.schema_name ORDER BY s.schema_name"
            }
            _ => {
                "SELECT datname, pg_database_size(datname) FROM pg_database \
                 WHERE NOT datistemplate ORDER BY datname"
            }
        };
        Ok(parse_list(&self.sql(sql, None)?))
    }

    pub fn create(&self, name: &str) -> Result<(), String> {
        check(name)?;
        match self.kind {
            ServiceKind::Mysql => {
                self.sql(&format!("CREATE DATABASE IF NOT EXISTS `{name}`"), None)?;
            }
            _ => {
                let exists = self.sql(
                    &format!("SELECT 1 FROM pg_database WHERE datname = '{name}'"),
                    None,
                )?;
                if exists.trim() != "1" {
                    self.sql(&format!("CREATE DATABASE \"{name}\""), None)?;
                }
            }
        }
        Ok(())
    }

    pub fn drop(&self, name: &str) -> Result<(), String> {
        check(name)?;
        if SYSTEM.contains(&name) {
            return Err(format!(
                "{name} belongs to the server and cannot be dropped"
            ));
        }
        match self.kind {
            ServiceKind::Mysql => self.sql(&format!("DROP DATABASE IF EXISTS `{name}`"), None)?,
            _ => self.sql(
                &format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"),
                None,
            )?,
        };
        Ok(())
    }

    /// Dump `name` into a gzipped SQL file in `dir`; returns it.
    pub fn backup(&self, name: &str, dir: &Path) -> Result<BackupInfo, String> {
        check(name)?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let created = red_core::time::unix_now();
        let file = dir.join(backup_name(name, created));
        let mut command = self.command(&self.dump);
        match self.kind {
            ServiceKind::Mysql => {
                command.args([
                    "--single-transaction",
                    "--routines",
                    "--triggers",
                    "--events",
                    name,
                ]);
            }
            _ => {
                command.args(["--no-owner", "--no-acl", "-d", name]);
            }
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run {}: {e}", self.dump.display()))?;
        let partial = file.with_extension("gz.partial");
        let result = (|| {
            let out = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
            let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::default());
            let mut stdout = child.stdout.take().expect("piped");
            std::io::copy(&mut stdout, &mut gz).map_err(|e| e.to_string())?;
            gz.finish().map_err(|e| e.to_string())?;
            let mut stderr = String::new();
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut stderr);
            }
            let status = child.wait().map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("the dump failed: {}", stderr.trim()));
            }
            Ok(())
        })();
        if let Err(err) = result {
            let _ = std::fs::remove_file(&partial);
            return Err(err);
        }
        std::fs::rename(&partial, &file).map_err(|e| e.to_string())?;
        let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        Ok(BackupInfo {
            file: file.display().to_string(),
            database: name.to_owned(),
            size,
            created,
        })
    }

    /// Feed a `.sql` or `.sql.gz` file into `name`.
    pub fn restore(&self, name: &str, file: &Path, create: bool) -> Result<(), String> {
        check(name)?;
        if create {
            self.create(name)?;
        }
        let input = std::fs::File::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let mut reader: Box<dyn Read> = if file.extension().is_some_and(|e| e == "gz") {
            Box::new(flate2::read::GzDecoder::new(input))
        } else {
            Box::new(input)
        };
        let mut command = self.command(&self.client);
        match self.kind {
            ServiceKind::Mysql => {
                command.arg(name);
            }
            _ => {
                command.args(["-d", name, "-v", "ON_ERROR_STOP=1", "-q", "-f", "-"]);
            }
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run {}: {e}", self.client.display()))?;
        let mut stdin = child.stdin.take().expect("piped");
        let copied = std::io::copy(&mut reader, &mut stdin);
        let _ = stdin.flush();
        drop(stdin);
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "the restore failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        copied.map_err(|e| format!("reading {}: {e}", file.display()))?;
        Ok(())
    }
}

fn check(name: &str) -> Result<(), String> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(format!(
            "{name:?} is not a database name RED handles; use letters, digits, '_' and '-'"
        ))
    }
}

fn run(mut command: Command) -> Result<String, String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `name<TAB>size` lines, without the server's own databases.
fn parse_list(text: &str) -> Vec<DatabaseInfo> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next()?.trim();
            if name.is_empty() || SYSTEM.contains(&name) {
                return None;
            }
            Some(DatabaseInfo {
                name: name.to_owned(),
                size: parts.next().and_then(|s| s.trim().parse().ok()),
            })
        })
        .collect()
}

/// `shop-20261019-153000.sql.gz` (UTC).
fn backup_name(database: &str, unix: u64) -> String {
    let date = red_core::time::datetime_from_unix(unix);
    // `2026-10-19 15:30:00Z` → `20261019-153000`
    let stamp: String = date
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == ' ')
        .collect::<String>()
        .replace(' ', "-");
    format!("{database}-{stamp}.sql.gz")
}

/// Backups in `dir`, newest first.
pub fn backups(dir: &Path) -> Vec<BackupInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<BackupInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".sql.gz")?;
            // `<db>-YYYYMMDD-HHMMSS`
            let database = stem.get(..stem.len().checked_sub(16)?)?.to_owned();
            let meta = entry.metadata().ok()?;
            Some(BackupInfo {
                file: entry.path().display().to_string(),
                database,
                size: meta.len(),
                created: meta
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_secs(),
            })
        })
        .collect();
    found.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.file.cmp(&a.file)));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_names() {
        assert!(valid_name("shop"));
        assert!(valid_name("my_app-2"));
        assert!(!valid_name("a b"));
        assert!(!valid_name("x`; drop"));
        assert!(!valid_name(""));
    }

    #[test]
    fn parses_database_lists() {
        let list = parse_list("information_schema\t0\nshop\t16384\nmysql\t1\nblog\tNULL\n");
        assert_eq!(
            list,
            vec![
                DatabaseInfo {
                    name: "shop".into(),
                    size: Some(16384)
                },
                DatabaseInfo {
                    name: "blog".into(),
                    size: None
                },
            ]
        );
    }

    #[test]
    fn names_and_finds_backups() {
        let name = backup_name("my-shop", 1_792_418_400);
        assert!(name.starts_with("my-shop-2026"), "{name}");
        assert!(name.ends_with(".sql.gz"));
        assert_eq!(name.len(), "my-shop".len() + 1 + 15 + 7, "{name}");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(&name), b"x").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        let found = backups(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].database, "my-shop");
        assert_eq!(found[0].size, 1);
        assert!(backups(&dir.path().join("missing")).is_empty());
    }
}
