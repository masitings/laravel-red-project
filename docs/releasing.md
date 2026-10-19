# Releasing RED

A release is a `v*` tag. Pushing one runs `.github/workflows/release.yml`,
which builds:

- `red-<tag>-<target>.tar.gz`: `red`, `redd` and `red-helper` for macOS
  (Apple Silicon, Intel) and Linux x86_64;
- `RED-<tag>-macos-<arch>.dmg`: the desktop app, with the three binaries
  inside;

and publishes them, with SHA-256 files and the CHANGELOG section for the
version, on a GitHub release.

## Steps

1. Move the `[Unreleased]` section of `CHANGELOG.md` under a new
   `## [x.y.z] - YYYY-MM-DD` heading.
2. Set the version in `Cargo.toml` (`[workspace.package]`),
   `apps/gui/package.json` and `apps/gui/src-tauri/tauri.conf.json`, then run
   `cargo check` so `Cargo.lock` follows.
3. Commit, tag and push:

   ```sh
   git commit -am "release x.y.z"
   git tag vx.y.z
   git push && git push --tags
   ```

The runtimes (PHP, Valkey) are released separately by the Runtimes workflow;
see the README.

## Optional: signing, notarization, auto-update, Homebrew

Each of these turns on when its secrets exist (Settings → Secrets and
variables → Actions). Without them the release still works: binaries are
ad-hoc signed, the app does not update itself, and no tap is touched.

### Developer ID signing and notarization

Needs a paid Apple Developer account. Without it, macOS asks people to
allow RED in System Settings → Privacy & Security the first time they open
it.

| Secret | Value |
|--------|-------|
| `APPLE_CERTIFICATE` | Base64 of a *Developer ID Application* certificate exported as `.p12` (`base64 -i cert.p12 \| pbcopy`) |
| `APPLE_CERTIFICATE_PASSWORD` | The `.p12` export password |
| `APPLE_ID` | Apple ID email used for notarization |
| `APPLE_PASSWORD` | An app-specific password for that Apple ID (appleid.apple.com) |
| `APPLE_TEAM_ID` | The 10-character team ID |

With the first two, the app, the disk image and the command-line tools are
signed with hardened runtime; with all five they are also notarized (the
app is stapled; bare executables are checked online by Gatekeeper).

### App auto-update

The app checks `releases/latest/download/latest.json` every six hours and
installs signed update archives. Updates are signed with a key of their own
(not the Apple certificate). Create it once:

```sh
cd apps/gui
pnpm tauri signer generate -w ~/.tauri/red-updater.key
```

| Kind | Name | Value |
|------|------|-------|
| Secret | `TAURI_SIGNING_PRIVATE_KEY` | Contents of `~/.tauri/red-updater.key` |
| Secret | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password you chose (empty if none) |
| Variable | `TAURI_UPDATER_PUBKEY` | Contents of `~/.tauri/red-updater.key.pub` |

Keep the private key safe: apps already installed only accept updates
signed with it. Builds without these (local builds, forks) have no updater;
their Status page says so.

After an update the app restarts; if the running redd is older than the app
(redd keeps running across app updates), the Status page offers to restart
it.

### Homebrew tap

Create a public repository named `homebrew-red` (or set the variable
`HOMEBREW_TAP_REPO` to `owner/name`), then add:

| Secret | Value |
|--------|-------|
| `HOMEBREW_TAP_TOKEN` | A fine-grained token with *Contents: read and write* on the tap repository |

Each release then writes `Formula/red.rb` (command-line tools) and
`Casks/red.rb` (the app) there:

```sh
brew install --cask masitings/red/red
brew install masitings/red/red
```
