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

# Build the libretro core (cdylib)
build-libretro:
    cargo build --release -p zeff-libretro

# Install the libretro core into RetroArch (Windows)
# Usage: just install-libretro "C:\RetroArch-Win64"
[windows]
install-libretro retroarch_dir:
    cargo build --release -p zeff-libretro
    Copy-Item "target\release\zeff_libretro.dll" "{{retroarch_dir}}\cores\zeff_libretro.dll" -Force
    Copy-Item "crates\zeff-libretro\zeff_libretro.info" "{{retroarch_dir}}\info\zeff_libretro.info" -Force
    Write-Host "Installed zeff_libretro.dll and .info to {{retroarch_dir}}"

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

# Run all tests with nextest (parallel, isolated)
test-nextest:
    cargo nextest run --workspace

# Run tests with both feature sets (matches CI)
test-all:
    cargo nextest run --workspace
    cargo nextest run --workspace --no-default-features

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Check without building
check:
    cargo check --workspace

# Format all code
fmt:
    cargo fmt --all

# Check formatting (CI-style, no changes)
fmt-check:
    cargo fmt --all -- --check

# Run Clippy lints (deny warnings, all targets & features)
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run Clippy lints with no default features
lint-minimal:
    cargo clippy --workspace --all-targets --no-default-features -- -D warnings

# Run Clippy lints for WASM target
lint-wasm:
    cargo clippy --target wasm32-unknown-unknown --no-default-features -- -D warnings

# Run Clippy lints with all feature sets (matches CI)
lint-all: lint lint-minimal lint-wasm

# Check that no native-only APIs leaked into shared code
[unix]
lint-platform-leaks:
    ! grep -rn --include='*.rs' -E '(rfd::|gilrs::|cpal::|dirs::|open::that|ureq::|pollster::block_on|nokhwa::)' src/ --exclude-dir=platform --exclude-dir=input --exclude-dir=audio --exclude='cli/*' | grep -v '// platform-ok'

[windows]
lint-platform-leaks:
    $hits = Get-ChildItem -Path src -Recurse -Filter *.rs | Where-Object { $_.FullName -notmatch '\\(platform|input\\native|audio\\native|audio\\tests|camera|cli|mods\\native|libretro_common)' -and $_.Name -ne 'native.rs' } | Select-String -Pattern 'rfd::|gilrs::|cpal::|dirs::|open::that|ureq::|pollster::block_on|nokhwa::' | Where-Object { $_.Line -notmatch '// platform-ok' }; if ($hits) { $hits; exit 1 } else { Write-Host 'No platform leaks found.' }

# Run full CI pipeline locally (fmt + lint + platform check + test + deny)
ci-local: fmt-check lint-all lint-platform-leaks test-all deny

# Run WASM CI check locally (requires wasm32 target: rustup target add wasm32-unknown-unknown)
ci-local-wasm: lint-wasm check-wasm

# Check that fuzz targets compile (requires nightly)
fuzz-check:
    cargo +nightly check --manifest-path fuzz/Cargo.toml

# Run criterion benchmarks
bench:
    cargo bench --workspace

# Audit dependencies for vulnerabilities and license issues (requires cargo-deny)
deny:
    cargo deny check

# Concatenate all src/*.rs files to clipboard (cross-platform alternative to scripts/get-all-code.ps1)
[unix]
get-all-code:
    find src -name '*.rs' | sort | while read f; do echo "// ===== $f ====="; cat "$f"; done | xclip -selection clipboard || echo "(xclip not available — output printed to stdout)"

[windows]
get-all-code:
    $allCode = ""; Get-ChildItem -Path src -Recurse -Filter *.rs | Sort-Object FullName | ForEach-Object { $allCode += "`n// ===== $($_.FullName) =====`n"; $allCode += Get-Content $_.FullName -Raw }; Set-Clipboard -Value $allCode

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

# Run the core profiling harness (3000 frames per ROM from manifests)
profile-cores:
    cargo run --profile profiling --bin profile_cores --features profile-cores

# Generate a flamegraph from the core profiling harness (requires admin on Windows)
flamegraph-cores:
    cargo flamegraph --profile profiling --bin profile_cores --features profile-cores -o flamegraph.svg

# Run Criterion benchmarks for GB core
bench-gb:
    cargo bench --bench gb_benchmarks -p zeff-gb-core

# Run Criterion benchmarks for NES core
bench-nes:
    cargo bench --bench nes_benchmarks -p zeff-nes-core

# Run all Criterion benchmarks (GB + NES)
bench-all: bench-gb bench-nes

# ──────────────────────────── Cleaning ───────────────────────────────

# Verify a release build compiles
release-check:
    cargo build --release

# Clean build artifacts
clean:
    cargo clean

# Clean and rebuild in profiling mode
clean-profile: clean build-profile

# ──────────────────────────── WASM / Web ─────────────────────────────

# Check WASM target compiles
check-wasm:
    cargo check --target wasm32-unknown-unknown --no-default-features

# Build WASM via Trunk (debug)
build-wasm:
    trunk build

# Build WASM via Trunk (release, optimized)
build-wasm-release:
    trunk build --release

# Build WASM exactly as GitHub Pages does (with public-url prefix)
build-wasm-ghpages:
    trunk build --release --public-url /zeff-boy/

# Serve WASM locally with hot-reload (open http://localhost:8080)
serve-wasm:
    trunk serve

# Serve WASM release build locally (reproduces GitHub Pages conditions)
serve-wasm-release:
    trunk serve --release

# ──────────────────────────── Documentation ──────────────────────────

# Generate documentation
doc:
    cargo doc --workspace --no-deps --open

