# RED Project

A fast, open-source local PHP development environment for macOS and Linux.
Park a folder, open `https://my-app.test`. No Docker, no hand-written vhosts.

> **Status: M0 (foundation).** The daemon, CLI and GUI talk to each other
> through the Control API, but RED does not serve sites yet. Serving sites
> over HTTPS with per-site PHP versions arrives in M1 (v0.1.0). See the
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

## Architecture

| Component    | What it does                                                         |
|--------------|----------------------------------------------------------------------|
| `redd`       | User-level daemon: Control API (unix socket), and from M1 the web server, DNS responder and process supervisor |
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
just daemon         # run redd in the foreground
just status         # in another terminal: red status
just gui            # run the desktop app against the running redd
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
| M0 Foundation | Workspace, CI, Control API, GUI shell | Green build on macOS + Linux |
| M1 MVP | Park/link, `.test` DNS, local HTTPS, multiple PHP versions, proxies with WebSocket, GUI for sites and PHP | v0.1.0 |
| M2 Dev tools | MySQL/PostgreSQL/Valkey, mail catcher, dumps, log viewer, `red.yml`, Node per site | v0.2.0 |
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
