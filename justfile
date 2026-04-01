# zeff-boy task runner
# Install: cargo install just  (or: winget install Casey.Just)
# Usage:   just <recipe>        (run `just --list` to see all recipes)

# Use PowerShell on Windows
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# Default recipe: list available commands
default:
    @just --list

# ──────────────────────────── Development ────────────────────────────

# Build in debug mode
build:
    cargo build

# Build in release mode
build-release:
    cargo build --release

# Build without optional features (no camera, no OGG recording)
build-minimal:
    cargo build --no-default-features

# Run the emulator (debug) with a ROM
run rom:
    cargo run -- "{{rom}}"

# Run the emulator (release) with a ROM
run-release rom:
    cargo run --release -- "{{rom}}"

# Run the emulator without optional features
run-minimal rom:
    cargo run --no-default-features -- "{{rom}}"

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Check without building
check:
    cargo check --workspace

# ──────────────────────────── Profiling ──────────────────────────────

# Build with profiling profile (debug symbols, optimized)
build-profile:
    cargo build --profile profiling

# Run the emulator in profiling mode with a ROM
run-profile rom:
    cargo run --profile profiling -- "{{rom}}"

# Run headless for N frames (default 600):useful for benchmarking
run-headless rom frames="600":
    cargo run --profile profiling -- --headless --max-frames {{frames}} "{{rom}}"

# Run headless with APU disabled:fastest profiling path
run-headless-no-apu rom frames="600":
    cargo run --profile profiling -- --headless --no-apu --max-frames {{frames}} "{{rom}}"

# Generate a flamegraph (requires `cargo install flamegraph`)
# On Windows: needs dtrace or use Tracy/perf instead
flamegraph rom frames="1800":
    cargo flamegraph --profile profiling -- --headless --no-apu --max-frames {{frames}} "{{rom}}"

# Generate a flamegraph with custom output name
flamegraph-named rom name frames="1800":
    cargo flamegraph --profile profiling -o "{{name}}.svg" -- --headless --no-apu --max-frames {{frames}} "{{rom}}"

# ──────────────────────────── Cleaning ───────────────────────────────

# Clean build artifacts
clean:
    cargo clean

# Clean and rebuild in profiling mode
clean-profile: clean build-profile

# ──────────────────────────── Documentation ──────────────────────────

# Generate documentation
doc:
    cargo doc --workspace --no-deps --open

