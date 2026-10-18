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

RED uses the PHP already on your machine, so install PHP first
(`brew install php`, plus `php@8.3` etc. for more versions).

Download `RED-<version>-macos-<arch>.dmg` from the
[releases page](https://github.com/masitings/red-project/releases) and drag
RED to Applications; the app contains the CLI too ("Install the red command"
on its Status page links it into `~/.local/bin`). Or download the archive
for your platform, or build from source with `just install`. Keep `red`,
`redd` and `red-helper` in the same directory, then run once:

```sh
red setup     # installs the helper (80/443), starts redd at login,
              # sets up .test DNS and trusts the local CA
red doctor    # confirms everything is in place
```

`red setup` asks for your password: the helper and the resolver file live
in system directories, and trusting the CA changes your keychain. Each
step can also be run on its own (`red helper:install`, `red service:install`,
`red dns:install`, `red trust`).

## Usage

```sh
cd ~/Sites && red park          # every sub-directory becomes <name>.test
cd ~/code/shop && red link      # or link a single project
red sites                       # URL, driver, PHP version, HTTPS
red which                       # which site and driver serve this directory

red php:list                    # PHP versions found (Homebrew, system)
red use 8.4                     # default PHP for all sites
red isolate 8.2                 # pin the current site to PHP 8.2
red unisolate

red secure                      # serve the current site over HTTPS
red secured                     # secured sites and certificate expiry

red proxy vite 5173             # vite.test → your dev server, HMR included
red proxies
red unproxy vite

red doctor                      # what is set up, and how to fix what isn't
red events                      # watch what the daemon is doing
```

### Databases and caches

RED runs MySQL or MariaDB, PostgreSQL, and Redis or Valkey from the
binaries already on your machine (`brew install mysql postgresql@17 valkey`),
each with its own data directory, port and log, bound to 127.0.0.1.

```sh
red services:add mysql          # or mariadb, postgres, redis, valkey
red services:add postgres --name pg15 --server-version 15 --port 5433
red services                    # status, plus the servers RED found
red services:stop mysql         # also :start, :restart, :remove
red services:env mysql          # DB_* lines for your .env
```

Services start with redd (unless added with `--no-autostart`) and are
restarted if they crash. MySQL/MariaDB and PostgreSQL get a `root` user with
no password, like Laravel's defaults.

### Mail, dumps and logs

```sh
red mail                        # mail your apps sent (SMTP on 127.0.0.1:2525)
red mail:show <id>
red dumps:on                    # dump()/dd() output goes to RED, not the page
red dumps --follow
red logs                        # RED, service, PHP-FPM and site logs
red logs site:blog:storage/logs/laravel.log -f
```

Point Laravel at the mail catcher with `MAIL_MAILER=smtp`, `MAIL_HOST=127.0.0.1`,
`MAIL_PORT=2525`; the SMTP username becomes the mailbox, so apps stay apart.

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

Every command accepts `--json`. The desktop app offers the same: sites, PHP
versions, proxies, services, mail, dumps, logs and the doctor checks.

RED's CA is restricted (X.509 Name Constraints) to `.test` and
`localhost`, so it cannot issue certificates for real domains.

Supported out of the box: Laravel (and Statamic), Symfony, Craft, Drupal,
WordPress, Bedrock, any project with `public/index.php`, plain PHP, and
static sites.

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
| M3 Distribution | Linux, our own static PHP builds, signed and notarized releases | Signed releases on both OSes |
| M4 Advanced | On-demand Xdebug, sharing/tunnels, profiler, MCP server, git workspaces | v1.0.0 |

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
