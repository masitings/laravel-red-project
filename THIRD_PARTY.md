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
