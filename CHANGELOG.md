# Changelog

All notable changes to RED Project. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Xdebug
- Xdebug on demand: sites run in a PHP-FPM pool without Xdebug, and RED
  starts a second pool per PHP version with Xdebug loaded for requests that
  ask for it: the `XDEBUG_SESSION` / `XDEBUG_TRIGGER` / `XDEBUG_PROFILE` /
  `XDEBUG_TRACE` cookies, or `XDEBUG_SESSION_START` / `XDEBUG_TRIGGER` in
  the query. `red xdebug:on` sends every request there, `red xdebug:off`
  none; `red xdebug` shows the mode and which PHP versions have Xdebug.
- `red php` loads Xdebug when it is on, or on demand with `XDEBUG_TRIGGER`,
  `XDEBUG_SESSION` or `XDEBUG_CONFIG` set.
- Xdebug connects to 127.0.0.1 on port 9003 (`xdebug_port` in
  `config.toml`); profiles and traces go to RED's log directory. Responses
  from the Xdebug pool carry an `X-RED-Xdebug: 1` header.
- RED's macOS PHP builds now include `xdebug.so` (the Runtimes workflow
  builds it as a shared extension and ships PHP without it if that fails).
  Other installations are used when their extension directory has
  `xdebug.so`; one whose `php.ini` already loads Xdebug is left as it is.
- The app's PHP page has an Xdebug switch (Off, On demand, Always) and
  shows which versions have Xdebug; `red doctor` reports it.
- Runtimes remember the checksum of the archive they came from, so a
  rebuilt archive of the same version (such as PHP with Xdebug) is offered
  as an update ("new build").

### MCP server
- `red mcp` serves RED to AI coding agents (Claude Code, Cursor, Zed, …) as
  a Model Context Protocol server on stdio (protocol 2025-11-25, and
  2025-06-18, 2025-03-26 and 2024-11-05 for older clients). Its tools wrap
  the Control API: status and doctor; sites (list, find the site for a
  directory, link, secure); PHP versions and the Xdebug mode; logs; caught
  mail (bodies as plain text); dumps (as text, `after_id` for new ones);
  services (list, start, stop, restart); runtimes and proxies.
- Tools carry read-only/destructive hints, return structured content plus a
  text copy, and report a stopped redd as a tool error with how to start it.
- Caught HTML mail and dumps read better as text: headings and list items
  end a line.

## [0.3.0] - 2026-10-19

Self-contained runtimes and updates: RED downloads PHP, databases, Node.js
and Composer itself, keeps them up to date, and the release pipeline can
sign, notarize and publish to Homebrew. Linux gets CA trust, DNS setup,
doctor checks and its own packages.

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

### Updates
- Runtimes: `red runtimes` shows an UPDATE column and `red runtimes:update
  [runtime] [line]` installs the newest build of each line. A PHP that came
  from static-php-cli's fallback builds is offered RED's own build of the
  same version (it has pdo_sqlite and pdo_pgsql). redd checks every six
  hours and announces updates; set `runtimes_auto_update = true` in
  `config.toml` to install patch updates automatically.
- Updating a runtime that a service runs stops the service, swaps the build
  (the old one is put back if that fails) and starts it again.
- The app's Status page has an Updates card: runtime updates with "Update
  all", the app's own updates, and a notice with a "Restart redd" button
  when the running redd is older than the app. The Status tab shows how
  many updates are waiting.
- The desktop app updates itself (Tauri's updater, signed update archives
  and `latest.json` on each release) when the release was built with an
  update signing key; other builds say so and do not try.

### Releases
- The release workflow signs and notarizes the app, the disk image and the
  command-line tools when Developer ID secrets are configured (ad-hoc
  signing otherwise), builds the update archives and `latest.json` when an
  updater key is configured, and updates a Homebrew tap (formula for the
  command-line tools, cask for the app) when a tap token is configured.
  See `docs/releasing.md`.

### Other
- `red uninstall` undoes `red setup` in one go: the resolver file, CA trust
  (keychain, or the Linux system store and browser databases), starting at
  login (stopping redd and its services, also when started by hand), the
  helper and RED's commands in `~/.local/bin`. `--purge` also deletes RED's
  data; project folders are never touched. It carries on past steps that
  fail and lists them at the end.
- The app's "Install the red command" copies red, redd and red-helper into
  `~/.local/bin` when it runs as an AppImage (whose files move on every
  launch) and refreshes the copies when it starts; elsewhere it still links
  to the app.
- `red restart` restarts redd through its login agent (after installing a
  new version, for example).
- When port 80 or 443 is taken (Herd, Valet, another server), redd serves
  on 8080/8443 instead of not serving at all, and `red doctor` names what
  holds the port and whether `/etc/resolver/test` belongs to another tool.

### Linux
- `red trust` and `red untrust` work on Linux: the system store (Debian and
  Ubuntu through `update-ca-certificates`, Fedora, Arch and openSUSE through
  `trust anchor`) and the NSS databases of Chrome and every Firefox profile
  (including Snap and Flatpak) through `certutil`.
- `red dns:install` without the helper writes the systemd-resolved drop-in
  itself and restarts systemd-resolved; without systemd-resolved it says
  how to point `.test` at redd with dnsmasq. `red dns:uninstall` removes
  only RED's own file.
- `red doctor` checks on Linux: the resolver drop-in and its port, whether
  `/etc/resolv.conf` goes through systemd-resolved, whether systemd is new
  enough (246+) for a DNS port other than 53, and whether the system and
  each browser trust RED's CA.
- Releases include the command-line tools for Linux arm64, and the desktop
  app as `.deb` (x86_64, arm64) and AppImage (x86_64, which can update
  itself). Linux binaries are built on Ubuntu 22.04 so they run on older
  distributions; the Homebrew formula covers Linux arm64.
- redd's PATH includes Homebrew on Linux (`/home/linuxbrew/.linuxbrew`).

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
