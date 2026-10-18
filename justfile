# Task runner for RED Project. Run `just` to list recipes.

gui := "apps/gui"

default:
    @just --list

# Install frontend dependencies
setup:
    cd {{gui}} && pnpm install --frozen-lockfile

# Run redd in the foreground (RED_HOME=/tmp/red keeps it separate from an install)
daemon:
    cargo run -p red-daemon --bin redd

# Show daemon status through the CLI
status:
    cargo run -p red-cli --bin red -- status

# Run the desktop app in development mode
gui:
    cd {{gui}} && pnpm tauri dev

# Build RED.app (macOS) or the Linux packages, with red, redd and red-helper inside
bundle:
    cargo build --release -p red-cli -p red-daemon -p red-helper
    mkdir -p {{gui}}/src-tauri/binaries
    for bin in red redd red-helper; do cp target/release/$bin {{gui}}/src-tauri/binaries/$bin-$(rustc -vV | sed -n 's/^host: //p'); done
    cd {{gui}} && pnpm tauri build --config src-tauri/tauri.bundle.conf.json

# Regenerate TypeScript types from crates/red-api
gen-types:
    cargo test -p red-api export_bindings

# Format Rust and frontend code
fmt:
    cargo fmt --all
    cd {{gui}} && pnpm format

# Everything CI checks
check: check-rust check-gui

check-rust:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

check-gui:
    cd {{gui}} && pnpm lint && pnpm typecheck && pnpm test

# Build release binaries and install red, redd and red-helper into PREFIX/bin
install prefix=(env_var("HOME") + "/.local"):
    cargo build --release -p red-cli -p red-daemon -p red-helper
    mkdir -p {{prefix}}/bin
    install -m 0755 target/release/red target/release/redd target/release/red-helper {{prefix}}/bin/
    @echo "Installed to {{prefix}}/bin."
    @# Restart a running redd so the new version takes over (ignored before `red setup`).
    -@test -f "$HOME/Library/LaunchAgents/dev.redproject.redd.plist" -o -f "$HOME/.config/systemd/user/redd.service" && {{prefix}}/bin/red restart
    @echo "First install? Run 'red setup' next."
