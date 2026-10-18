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
