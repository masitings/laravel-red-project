# ADR-0001: Overall architecture

- Status: accepted
- Date: 2026-10-18

## Context

RED Project is an open-source local PHP development environment, built
clean-room (see `CONTRIBUTING.md`). It targets macOS first and Linux next,
with a Rust core and a Tauri 2 + React desktop app.

## Decisions

1. **One user-level daemon (`redd`) owns all state.** The CLI, the GUI and a
   future MCP server are thin clients of its Control API, so every client
   behaves the same.
2. **Control API = HTTP/1.1 + JSON (and SSE for events) over a unix socket.**
   No TCP port. The socket is `0600` inside a `0700` directory and the
   daemon checks the peer's uid. Paths are versioned (`/v1/...`). Types live
   in `red-api`; TypeScript types for the GUI are generated with `ts-rs`.
3. **Own web server and DNS responder in Rust** (hyper + rustls, and a small
   `.test`-only DNS responder) instead of bundling nginx and dnsmasq. One
   binary, routing changes without reloads, and WebSocket proxying and
   PHP-FPM self-healing in the request path.
4. **Minimal root helper.** `red-helper` only opens loopback ports 80/443
   (passing the fds to `redd`) and writes the resolver file. DNS runs on a
   high port as the user (macOS `/etc/resolver` supports a `port` option).
5. **OS-conventional file locations**: `~/Library/Application Support/RED`
   on macOS, XDG directories on Linux, `~/.red/bin` for CLI shims only.
   `RED_HOME` overrides everything for tests and development builds.
6. **License: `MIT OR Apache-2.0`.**

## Consequences

- RED must reproduce the few nginx behaviours PHP apps rely on through
  explicit driver rules (static files vs front controller).
- PHP distribution is staged: use existing PHP installs first (M1), our own
  static builds later (M3).
