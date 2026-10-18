# Contributing to RED Project

Thanks for helping out! This document covers the ground rules, how to get a
working setup, and what we expect from a pull request.

## Clean-room rules (required)

RED is an independent implementation. To keep it that way, every
contribution must follow these rules:

1. **Allowed sources:** public documentation of other tools (to learn *what*
   a feature does, not *how* it is built), open-source code under a
   compatible license (credited in `THIRD_PARTY.md`), and public
   specifications (FastCGI, DNS, X.509, PHP and Symfony documentation, …).
2. **Not allowed:** reverse engineering, decompiling or disassembling
   proprietary software; copying its code, assets, icons, UI layouts,
   names or branding; or using material derived from any of these.
3. If you reuse open-source code, keep its license header, add it to
   `THIRD_PARTY.md`, and make sure the license is compatible with
   `MIT OR Apache-2.0`.

When in doubt, ask in an issue before writing code.

## Setup

See [Development](README.md#development) in the README. In short:

```sh
just setup
just check
```

## Pull requests

- Keep each PR focused on one change. Open an issue first for larger work.
- `just check` must pass: rustfmt, `clippy -D warnings`, all tests,
  and the frontend lint, typecheck and tests.
- If you change a type in `crates/red-api`, run `just gen-types` and commit
  the regenerated files in `apps/gui/src/api/gen`. CI fails if they are
  out of date.
- Write commit messages in the
  [Conventional Commits](https://www.conventionalcommits.org) style
  (`feat(daemon): …`, `fix(cli): …`). The changelog is generated from them.
- Sign off your commits (`git commit -s`) to certify the
  [Developer Certificate of Origin](https://developercertificate.org).
- Architecture decisions are recorded as ADRs in `docs/adr/`. A change that
  alters one needs a new ADR.

## Code guidelines

- No `unsafe` code; the workspace forbids it. If something truly needs it,
  discuss it in an issue first.
- Platform-specific code belongs in `red-platform`, not behind
  `cfg(target_os)` in other crates.
- The GUI holds no business logic. If the GUI needs something, add it to the
  Control API so the CLI gets it too.
- Errors shown to users say what went wrong and, where possible, what to do
  next (the `hint` field of `ApiError`).

## Reporting security issues

Please do not open public issues for security problems. Use GitHub's
private vulnerability reporting on the repository instead.
