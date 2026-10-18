# Changelog

All notable changes to RED Project. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

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
