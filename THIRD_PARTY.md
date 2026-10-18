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

Software RED downloads at the user's request (not shipped with RED):

| Software | Source | License |
|----------|--------|---------|
| PHP builds | Built by RED's CI with [static-php-cli](https://github.com/crazywhalecc/static-php-cli) (MIT); fallback: its prebuilt binaries (`dl.static-php.dev`) | PHP License; bundled libraries under their own licenses |
| Valkey | Built by RED's CI from [valkey-io/valkey](https://github.com/valkey-io/valkey) | BSD-3-Clause |
| Node.js | nodejs.org official builds | MIT (and bundled components' licenses) |
| Composer | getcomposer.org | MIT |
| PostgreSQL | [zonky embedded-postgres-binaries](https://github.com/zonkyio/embedded-postgres-binaries) on Maven Central | PostgreSQL License; packaging Apache-2.0 |
| MySQL | Oracle's official tarballs (`cdn.mysql.com/archives`) | GPL-2.0 with Oracle's exceptions |

Projects we may build on later:

| Project | Planned use | License |
|---------|-------------|---------|
| [Laravel Valet](https://github.com/laravel/valet) | Reference for framework detection rules | MIT |

Protocols RED speaks, implemented from their public documentation:

| Protocol | Where | Notes |
|----------|-------|-------|
| FastCGI | `crates/red-daemon/src/fastcgi.rs` | Talks to PHP-FPM |
| SMTP (RFC 5321) | `crates/red-daemon/src/mail.rs` | Mail catcher; parsing via the `mail-parser` crate |
| Symfony VarDumper server mode | `crates/red-daemon/src/dumps.rs` | Rendering runs the project's own `symfony/var-dumper` (MIT); none of its code ships with RED |
