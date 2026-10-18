# Third-party code and dependencies

RED Project contains no code copied from other projects. It depends on the
open-source libraries listed in `Cargo.lock` and `apps/gui/pnpm-lock.yaml`,
each under its own license. `cargo deny check licenses` (configured in
`deny.toml`) runs in CI and only allows licenses compatible with
`MIT OR Apache-2.0`.

When code from another project is adapted, record it here:

| Project | Used for | License | Location in this repo |
|---------|----------|---------|-----------------------|
| _none yet_ | | | |

Projects we plan to build on (not yet included):

| Project | Planned use | License |
|---------|-------------|---------|
| [static-php-cli](https://github.com/crazywhalecc/static-php-cli) | Building static PHP binaries (M3) | MIT |
| [Laravel Valet](https://github.com/laravel/valet) | Reference for framework detection rules | MIT |

Protocols RED speaks, implemented from their public documentation:

| Protocol | Where | Notes |
|----------|-------|-------|
| FastCGI | `crates/red-daemon/src/fastcgi.rs` | Talks to PHP-FPM |
| SMTP (RFC 5321) | `crates/red-daemon/src/mail.rs` | Mail catcher; parsing via the `mail-parser` crate |
| Symfony VarDumper server mode | `crates/red-daemon/src/dumps.rs` | Rendering runs the project's own `symfony/var-dumper` (MIT); none of its code ships with RED |
