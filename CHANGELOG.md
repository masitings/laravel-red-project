# Changelog

All notable changes to RED Project. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Runtimes
- RED downloads and manages PHP, MySQL, PostgreSQL, Valkey, Node.js and
  Composer itself, no Homebrew needed: `red php:install [8.4]`,
  `red node:install [22]`, `red runtimes`,
  `red runtimes:install <runtime> [version]`, `red runtimes:remove`.
  - PHP and Valkey: built by RED's own CI (the Runtimes workflow, using
    static-php-cli for PHP) and listed with checksums in a manifest on the
    `runtimes` release. RED's PHP builds include pdo_sqlite and pdo_pgsql,
    which static-php-cli's prebuilt binaries lack; those are only used for
    versions RED has not built yet. Each PHP has its own editable
    `php.ini`.
  - Node.js: official builds from nodejs.org (LTS lines and the newest),
    verified against `SHASUMS256.txt`. `red node`, `red npm` and `red npx`
    install the project's version on first use.
  - Composer: the official `composer.phar`, verified against its published
    checksum; `red composer` installs it on first use, and `red env` puts it
    on PATH.
  - PostgreSQL 14 to 18: relocatable builds from the zonky project on Maven
    Central, verified against the published SHA-256.
  - MySQL 8.4 LTS: Oracle's official tarballs, verified against checksums
    pinned in RED.
- Downloads run in the background with progress (CLI, app and events),
  unpack into a staging directory and are checked to run before they
  replace anything; updating a line keeps its `php.ini`.
- RED's runtimes are found before Homebrew's; services can use them right
  away, and a runtime that a service uses cannot be removed by accident.
- `red setup` installs PHP when none is found.
- `runtimes_manifest` in `config.toml` points RED at another manifest (or
  `""` turns RED's own builds off).
- `red doctor` warns about PHP versions without pdo_sqlite.
- The app's PHP page and Services page can install, update and remove
  runtimes.

### Fixed
- MySQL services accept `root` from 127.0.0.1 (MySQL's root account is
  `root@localhost`).

## [0.2.0] - 2026-10-18

Developer tools: databases, mail, dumps, logs and per-project settings.

### Services
- `red services:add` runs MySQL or MariaDB, PostgreSQL, and Redis or Valkey
  from binaries already installed (Homebrew, Postgres.app, system
  packages), each with its own data directory, port and log on 127.0.0.1.
- Data directories are initialised on first start (`root` user without a
  password); services start with redd, restart after a crash with a growing
  delay, and are marked failed (with the log's last lines) if they keep
  crashing or their port is taken.
- `red services`, `services:start|stop|restart|remove|env`; a doctor check
  reports failed services.

### Mail catcher
- SMTP server on 127.0.0.1:2525 (`smtp_port` in `config.toml`) that keeps
  every message: HTML, text, headers, attachments, Bcc via the envelope.
  The SMTP username becomes the mailbox, so apps stay apart.
- `red mail`, `mail:show`, `mail:clear`; the app shows mail with an unread
  count, inline images, and saves attachments or the raw `.eml`.

### Dumps
- Dump server on 127.0.0.1:9912 speaking Symfony VarDumper's server
  protocol. With `red dumps:on`, `dump()` and `dd()` output from sites goes
  to RED instead of the page, rendered by the project's own VarDumper.
- `red dumps [--follow]`, `dumps:off`, `dumps:clear`; a Dumps page in the app.

### Logs
- `red logs` lists RED's, services', PHP-FPM's and sites' logs
  (`storage/logs`, `var/log`, `logs`); Laravel/Monolog entries are split
  into time, level and message with their stack traces. `-f` follows.
- A Logs page in the app with level filter, search and live updates.

### Projects
- `red.toml` per project (`name`, `php`, `secure`, `node`, `services`).
  redd honours `php` immediately; `red init` creates the file and applies
  the rest, then prints the `.env` lines to use.
- `red php`, `red composer`, `red node`, `red npm`, `red npx` run the
  project's versions; `red env` prints PATH exports for the shell.

### Desktop app
- New Services, Mail, Dumps and Logs pages; a "Start RED now" button when
  redd is not running; the Status page can link the bundled `red` into
  `~/.local/bin`.
- Releases include `RED-<version>-macos-<arch>.dmg` with `red`, `redd` and
  `red-helper` inside the app (`just bundle` builds it locally).

### System integration
- `red restart` restarts redd through its login agent (installing the agent
  if needed); `just install` uses it so a new build takes over right away.

### Known limitations
- The app is ad-hoc signed, not notarized: the first time, open it from
  System Settings → Privacy & Security → "Open Anyway".
- Dumps from artisan and other CLI scripts need `VAR_DUMPER_FORMAT=server`
  in their environment.
- RED uses installed database servers; it does not download them yet.

### Fixed
- The GUI's lockfile resolves only packages at least three days old, so it
  installs under stricter pnpm `minimumReleaseAge` settings.

## [0.1.0] - 2026-10-18

First release: the core loop of a local PHP environment.

### Sites
- `red park` serves every sub-directory of a folder as `<name>.test`;
  `red link` serves a single project under any name. Subdomains fall back to
  their parent site, and `*.localhost` works without DNS setup.
- Framework detection for Laravel (and Statamic), Symfony, Craft, Drupal,
  WordPress, Bedrock, `public/` layouts, plain PHP and static sites.
- Safe routing: dotfiles (`.env`, `.git`) are never served, path traversal
  is rejected, `PATH_INFO` and directory redirects work as PHP apps expect.
- Responses stream: large files and long-running PHP output are sent as
  they are produced.

### PHP
- Uses the PHP-FPM already installed (Homebrew or system packages).
- One PHP-FPM pool per version, started on demand and restarted
  automatically if it crashes.
- `red use` sets the default version; `red isolate` pins a site.

### HTTPS and DNS
- Built-in DNS responder for `.test` on an unprivileged port, wired up with
  `red dns:install`.
- Local certificate authority restricted by X.509 Name Constraints to
  `.test` and `localhost`; `red secure` issues per-site certificates
  (renewed automatically), with HTTP/2 and HTTP→HTTPS redirects.
- `red trust` adds the CA to the macOS keychain.

### Proxies
- `red proxy <name> <port>` forwards a `.test` name to a local dev server,
  WebSocket upgrades included (Vite and webpack HMR, Next.js, Reverb).

### System integration
- `red-helper`, a minimal root helper, opens ports 80/443 and passes them to
  the unprivileged daemon; it accepts a fixed set of requests from allowed
  users only.
- `red service:install` starts `redd` at login (launchd / systemd user).
- `red setup` runs all one-time steps; `red doctor` checks PHP, ports, DNS,
  name resolution, CA trust, helper and autostart, with a fix for each.

### Interfaces
- Control API over a private unix socket, with Server-Sent Events for live
  updates. Every CLI command supports `--json`.
- Desktop app (Tauri 2 + React): sites, PHP versions, proxies, status and
  doctor checks, updated live.

### Known limitations
- Linux: the helper and DNS setup are implemented but less tested; CA trust
  is manual.
- The desktop app is not yet packaged or signed; run it from source.
- No database services, mail catcher or dump viewer yet (planned for 0.2).
