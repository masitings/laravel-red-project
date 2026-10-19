# RED Project

A fast, open-source local PHP development environment for macOS and Linux.
Park a folder, open `https://my-app.test`. No Docker, no hand-written vhosts.

> **Status: v0.2.0.** Sites, per-site PHP, local HTTPS, `.test` DNS,
> proxies with WebSockets, database services, a mail catcher, a dump
> viewer, a log viewer, per-project `red.toml` and a desktop app all work on
> macOS. Linux runs too, with some setup steps still manual. See the
> [roadmap](#roadmap).

## Why

- **Native and lightweight.** One Rust daemon runs the web server, DNS and
  process supervisor. PHP runs as your user, not as root.
- **Free developer tools.** Mail catcher, dump viewer, log viewer and
  database services are part of RED, not a paid add-on.
- **macOS first, Linux next.** Platform code lives in a single crate, so
  Linux support doesn't mean a rewrite.
- **Scriptable.** Everything the GUI does goes through the same Control API
  as the `red` CLI, and every command can print JSON.

## Install

RED brings its own PHP, Node.js, Composer, MySQL, PostgreSQL and Valkey:
`red php:install`, `red node:install`, `red runtimes:install <runtime>`, or
the PHP and Services pages in the app. `red setup` installs PHP when it
finds none, and `red composer`/`red node` install themselves on first use.
Homebrew installations work too, but are not needed.

With Homebrew (once the tap is published):

```sh
brew install --cask masitings/red/red   # the app, with the red command
brew install masitings/red/red          # or only the command-line tools
```

Or download `RED-<version>-macos-<arch>.dmg` from the
[releases page](https://github.com/masitings/laravel-red-project/releases) and drag
RED to Applications. On first launch the app walks you through setup, one
button per step: the helper for ports 80 and 443 (one password prompt),
starting at login, PHP, `.test` DNS, trusted HTTPS and the `red` command.
No terminal needed. It then lives in the menu bar: sites, services, mail and
logs one click away; closing the window keeps it running there.

Or download the archive for your platform, or build from source with
`just install`. Keep `red`, `redd` and `red-helper` in the same directory,
then run once:

```sh
red setup     # installs the helper (80/443), starts redd at login,
              # sets up .test DNS and trusts the local CA
red doctor    # confirms everything is in place
```

`red setup` asks for your password: the helper and the resolver file live
in system directories, and trusting the CA changes your keychain. Each
step can also be run on its own (`red helper:install`, `red service:install`,
`red dns:install`, `red trust`).

### Linux

Download `RED-<version>-linux-<arch>.deb` (installs the app and puts `red`,
`redd` and `red-helper` in `/usr/bin`), the x86_64 AppImage, or the
command-line archive, then run `red setup` as above. On Linux:

- the helper runs as a systemd service and redd as a systemd user service
  (`journalctl --user -u redd` for its log);
- `.test` DNS needs systemd-resolved 246 or newer (Ubuntu 22.04+, Debian 12+,
  Fedora); RED adds a drop-in in `/etc/systemd/resolved.conf.d`. Without
  systemd-resolved, point `.test` at redd yourself, e.g. dnsmasq's
  `server=/test/127.0.0.1#53530`;
- `red trust` adds the CA to the system store (`update-ca-certificates` or
  `trust anchor`) and to Chrome's and Firefox's certificate databases, which
  needs `certutil` (`libnss3-tools` on Debian/Ubuntu, `nss-tools` on
  Fedora);
- MySQL needs libaio (`sudo apt install libaio1t64` on Ubuntu 24.04).

`red doctor` checks each of these.

### Uninstall

```sh
red uninstall           # DNS, CA trust, start at login, helper, ~/.local/bin commands
red uninstall --purge   # …and RED's data: runtimes, databases, mail, certificates, logs
```

Your project folders are never touched. Then move RED.app to the Trash,
or remove the package (`brew uninstall red`, `sudo apt remove red`).

## Usage

```sh
cd ~/Sites && red park          # every sub-directory becomes <name>.test
cd ~/code/shop && red link      # or link a single project
red sites                       # URL, driver, PHP version, HTTPS
red which                       # which site and driver serve this directory

red php:install 8.4             # download PHP 8.4, managed by RED
red php:list                    # PHP versions found (RED's, Homebrew, system)
red use 8.4                     # default PHP for all sites
red isolate 8.2                 # pin the current site to PHP 8.2
red unisolate

red secure                      # serve the current site over HTTPS
red secured                     # secured sites and certificate expiry

red proxy vite 5173             # vite.test → your dev server, HMR included
red proxies
red unproxy vite

red doctor                      # what is set up, and how to fix what isn't
red doctor --fix                # fix what it can (asks for your password once)
red events                      # watch what the daemon is doing
```

### New projects

```sh
red new shop                     # Laravel in ./shop, served at shop.test
red new shop --kind react --db mysql --secure
red new blog --kind wordpress    # latest WordPress on your MySQL service
red new demo --kind blank        # just public/index.php
```

Kinds: `laravel` (default), `react`, `vue` and `livewire` (Laravel's
starter kits, frontend built with RED's Node.js), `wordpress` and `blank`.
RED creates the database in the service you name (Laravel uses SQLite
otherwise), points `.env` or `wp-config.php` at it and at RED's mail
catcher, and serves the folder: as part of a parked folder, or linked. The
app's Sites page does the same with **New site**, showing the output as it
runs.

### Settings and php.ini

The app's Settings page manages parked folders, opening RED at login, and
the mail, dump and Xdebug ports (in `config.toml`; applied after restarting
redd). On the PHP page, **Settings** per version sets `memory_limit`,
upload sizes, `max_execution_time`, `display_errors`, `date.timezone` and
any other php.ini value. RED passes them to PHP-FPM, `red php` and
`red composer` on top of the installation's own php.ini, so they work the
same for RED's builds, Homebrew and apt; it also lists the loaded
extensions.

### Databases and caches

RED runs MySQL or MariaDB, PostgreSQL, and Redis or Valkey, each with its
own data directory, port and log, bound to 127.0.0.1. It downloads MySQL,
MariaDB, PostgreSQL and Valkey itself, or uses what Homebrew installed.

```sh
red runtimes                    # what RED installed and can download
red runtimes:install postgres 18
red runtimes:install mysql
red runtimes:install valkey
red services:add mysql          # or mariadb, postgres, redis, valkey
red services:add postgres --name pg15 --server-version 15 --port 5433
red services                    # status, plus the servers RED found
red services:stop mysql         # also :start, :restart, :remove
red services:env mysql          # DB_* lines for your .env
red services:set mysql --port 3307 --server-version 8.4.6
red services:logs mysql -f
```

Services start with redd (unless added with `--no-autostart`) and are
restarted if they crash. MySQL/MariaDB and PostgreSQL get a `root` user with
no password, like Laravel's defaults. `services:set` changes the port, the
autostart flag or the server version (only newer versions of the same
server, so the data directory stays readable; PostgreSQL keeps its major
version) and restarts the service.

```sh
red db                          # databases and their size
red db:create shop
red db:backup shop              # gzipped dump in RED's data directory
red db:restore shop dump.sql.gz # .sql or .sql.gz, creates the database
red db:drop shop                # asks you to type the name
```

`--service` picks the service when there is more than one. The app's
Databases page does the same, with the list of backups. These commands use
the server's own client tools (`psql`, `pg_dump`, `mysql`, `mysqldump`),
falling back to ones installed elsewhere (Homebrew's `libpq` or
`mysql-client`, Postgres.app). RED's PostgreSQL builds include the
extensions Laravel apps tend to use (`uuid-ossp`, `pg_trgm`, `hstore`,
`citext`, `postgres_fdw`, …); pgcrypto needs OpenSSL and is not included.

### Sharing a site

`red share` puts a site on the internet through a
[Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/),
so a client or a webhook can reach it. RED downloads `cloudflared` the
first time.

```sh
red share                       # https://<random>.trycloudflare.com, no account
red share:login                 # once, to use a domain on your Cloudflare account
red share shop --hostname shop.example.com
red shares
red unshare shop
```

The site sees HTTPS (`HTTPS=on`) and its usual document root; the tunnel
stops when you unshare or redd stops. Quick links need no account and
change every time; with `--hostname` RED creates a named tunnel `red-<site>`
and the DNS record for it. In the app: a site's menu → **Share publicly**.

### Query inspector

```sh
red queries:on                  # capture SQL from every Laravel request
red queries -f                  # follow them: SQL with bindings, time, file:line
red queries:off
```

While capture is on, RED loads a small service provider into Laravel apps
through PHP's `auto_prepend_file` (nothing is written to the project) that
sends each query to redd. The app's Queries page groups them per request
and flags repeated queries (likely N+1) and slow ones (100 ms or more).

### Xdebug

Sites run without Xdebug, so they stay fast. With Xdebug on demand (the
default), a request that carries the `XDEBUG_SESSION` cookie (set by your IDE
or an Xdebug browser extension), or `XDEBUG_SESSION_START` / `XDEBUG_TRIGGER`
in its URL, runs in a second PHP-FPM pool with Xdebug loaded, which connects
to your IDE on port 9003.

```sh
red xdebug        # status per PHP version
red xdebug:on     # every request and every `red php` run
red xdebug:auto   # on demand (default)
red xdebug:off
XDEBUG_TRIGGER=1 red php artisan migrate   # debug a command
```

RED's macOS PHP builds include Xdebug. Other installations work when they
have `xdebug.so` in their extension directory (`pecl install xdebug`, the
`php-xdebug` packages); if their `php.ini` already loads it, it simply
stays on. The port is `xdebug_port` in `config.toml`; profiles and traces
(`XDEBUG_PROFILE`, `XDEBUG_TRACE`) go to RED's log directory under `xdebug/`.

### Mail, dumps and logs

```sh
red mail                        # mail your apps sent (SMTP on 127.0.0.1:2525)
red mail:show <id>
red dumps:on                    # dump()/dd() output goes to RED, not the page
red dumps --follow
red logs                        # RED, service, PHP-FPM and site logs
red logs site:blog:storage/logs/laravel.log -f
red logs service:mysql -f
```

Point Laravel at the mail catcher with `MAIL_MAILER=smtp`, `MAIL_HOST=127.0.0.1`,
`MAIL_PORT=2525`; the SMTP username becomes the mailbox, so apps stay apart.

### AI agents (MCP)

`red mcp` is a [Model Context Protocol](https://modelcontextprotocol.io)
server on stdio, so coding agents can see what RED sees: sites, logs, caught
mail, dumps, services, PHP and Xdebug. It talks to redd, so the app or the
login service must be running.

```sh
claude mcp add red -- red mcp          # Claude Code
```

Cursor, Zed, VS Code and other clients take the same command in their MCP
settings, for example in `.cursor/mcp.json`:

```json
{ "mcpServers": { "red": { "command": "red", "args": ["mcp"] } } }
```

Tools: `status`, `doctor`, `list_sites`, `site_for_path`, `link_site`,
`secure_site`, `php_versions`, `set_xdebug`, `list_logs`, `read_log`,
`list_mail`, `read_mail`, `clear_mail`, `list_dumps`, `clear_dumps`,
`list_services`, `service_action`, `create_project`, `list_projects`,
`list_runtimes`, `list_proxies`, `list_queries`, `set_query_capture`,
`list_databases`, `create_database`, `backup_database`, `list_shares`,
`share_site`, `stop_share`. Each is
marked read-only or not, so clients can ask before changes; nothing runs
outside RED's own API.

### Per-project settings

Commit a `red.toml` and everyone gets the same setup with `red init`:

```toml
name = "blog"                   # blog.test
php = "8.3"                     # redd picks this up as soon as you save
secure = true
node = "22"                     # for red node / npm / npx
services = ["mysql", "redis"]
```

`red init` creates the file on first run, then links the site, applies
HTTPS, starts the services and prints the `.env` lines to use. `red php`,
`red composer`, `red node`, `red npm` and `red npx` run the versions the
project asks for (Node.js from nvm, fnm, Volta, asdf, mise or Homebrew;
`.nvmrc` works too), and `eval "$(red env)"` puts them first on your PATH.
`red bun` and `red bunx` run the Bun RED downloaded
(`red runtimes:install bun`).

Every command accepts `--json`. The desktop app offers the same: sites, PHP
versions, proxies, services, databases, queries, sharing, mail, dumps, logs
and the doctor checks (with a Fix button), built with shadcn/ui and
following the system's light or dark mode.

RED's CA is restricted (X.509 Name Constraints) to `.test` and
`localhost`, so it cannot issue certificates for real domains.

Supported out of the box: Laravel (and Statamic), Symfony, Craft, Drupal,
WordPress, Bedrock, any project with `public/index.php`, plain PHP, and
static sites.

### Where the runtimes come from

| Runtime | Source |
|---------|--------|
| PHP | Built by RED's [Runtimes workflow](.github/workflows/runtimes.yml) with static-php-cli; static-php-cli's own builds for versions RED has not built yet |
| Valkey | Built by RED's Runtimes workflow |
| MariaDB | Built by RED's Runtimes workflow (macOS, Linux arm64); MariaDB's official tarballs on Linux x86_64 |
| PostgreSQL | Built by RED's Runtimes workflow, with `psql`, `pg_dump` and the contrib extensions; zonky's server-only builds on Maven Central for majors RED has not built (SHA-256 checked) |
| MySQL | Oracle's official tarballs (checksums pinned in RED) |
| Node.js | nodejs.org (SHA-256 checked) |
| Composer | getcomposer.org (SHA-256 checked) |
| Bun | Bun's GitHub releases (SHA-256 checked) |
| cloudflared | Cloudflare's GitHub releases (SHA-256 checked), for `red share` |

`red runtimes` shows which installed runtimes have a newer build and
`red runtimes:update` installs them (services using them restart on the new
build); the app's Status page does the same. redd looks for updates every
six hours; `runtimes_auto_update = true` in `config.toml` installs patch
updates as soon as they appear.

RED's builds are listed with their checksums in `manifest.json` on the
[`runtimes` release](https://github.com/masitings/laravel-red-project/releases/tag/runtimes).
Run the Runtimes workflow from the Actions tab to build them (it also runs
weekly to pick up patch releases).

## Architecture

| Component    | What it does                                                         |
|--------------|----------------------------------------------------------------------|
| `redd`       | User-level daemon: Control API (unix socket), web server (HTTP/1.1, HTTP/2, TLS), DNS responder, reverse proxy, PHP-FPM and service supervisor, SMTP mail catcher, dump server |
| `red`        | CLI for people and scripts                                           |
| RED GUI      | Tauri 2 + React desktop app, a thin client of the Control API        |
| `red-helper` | Tiny root helper that only opens ports 80/443 and writes the resolver file |

The workspace is split into these crates:

```
crates/red-core      domain logic: config, sites, drivers (no network IO)
crates/red-api       Control API types (+ TypeScript bindings via ts-rs) and client
crates/red-platform  macOS/Linux specifics: paths, users, resolver, trust store
crates/red-daemon    redd
crates/red-helper    red-helper
crates/red-cli       red
apps/gui             Tauri 2 shell (src-tauri) + React/Vite app (src)
```

## Development

Requirements: Rust (the toolchain is pinned in `rust-toolchain.toml` and
installed by rustup automatically), Node.js 22+, pnpm 10+, and
[just](https://github.com/casey/just). On Linux, the GUI also needs the
[Tauri system dependencies](https://tauri.app/start/prerequisites/).

```sh
just setup          # install frontend dependencies
just install        # build release binaries into ~/.local/bin
just daemon         # run redd in the foreground
just status         # in another terminal: red status
just gui            # run the desktop app against the running redd
just bundle         # build RED.app (or Linux packages) with the binaries inside
just check          # fmt, clippy, tests, frontend lint/typecheck/tests
```

To keep a development build away from an installed one, point it at a
scratch directory: `RED_HOME=/tmp/red just daemon`, and use the same
`RED_HOME` for `red` and the GUI.

Where RED keeps its files:

| | macOS | Linux |
|-|-------|-------|
| Config | `~/Library/Application Support/RED` | `$XDG_CONFIG_HOME/red` |
| Data | `~/Library/Application Support/RED/data` | `$XDG_DATA_HOME/red` |
| Socket | `~/Library/Application Support/RED/run/redd.sock` | `$XDG_RUNTIME_DIR/red/redd.sock` |
| Logs | `~/Library/Logs/RED` | `$XDG_STATE_HOME/red/logs` |

## Roadmap

| Phase | Scope | Gate |
|-------|-------|------|
| M0 Foundation ✓ | Workspace, CI, Control API, GUI shell | Green build on macOS + Linux |
| M1 MVP ✓ | Park/link, `.test` DNS, local HTTPS, multiple PHP versions, proxies with WebSocket, GUI for sites and PHP | v0.1.0 |
| M2 Dev tools ✓ | MySQL/PostgreSQL/Valkey, mail catcher, dumps, log viewer, `red.toml`, Node per site, desktop app bundle | v0.2.0 |
| M3 Distribution | Downloadable runtimes ✓, runtime and app updates ✓, signing/notarization and Homebrew tap in CI ✓, Linux DNS/trust/packages ✓ | Signed releases on both OSes |
| M3.5 Everyday app ✓ | Setup in the app, menu bar, new projects (Laravel, starter kits, WordPress), Settings, php.ini per version | v0.4.0 |
| M4 Advanced | On-demand Xdebug ✓, MCP server ✓, sharing/tunnels ✓, query inspector ✓, database tools ✓, profiler, git workspaces | v1.0.0 |

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md)
first, in particular the clean-room rules.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
