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

# Smoke the public C ABI with a synthetic HuCard in both pixel formats.
smoke-libretro-abi: build-libretro
    cargo run -p zeff-libretro --bin abi_smoke -- --pixel-format xrgb8888
    cargo run -p zeff-libretro --bin abi_smoke -- --pixel-format rgb565

# Fixed-frame ABI comparison; input is `frame:port:joypad-mask`.
libretro-harness core rom warmup="120" frames="600" format="xrgb8888" repeat="1":
    cargo run -p zeff-libretro --bin libretro_harness -- "{{core}}" "{{rom}}" --warmup {{warmup}} --frames {{frames}} --pixel-format {{format}} --repeat {{repeat}}

# Install the libretro core into RetroArch (Windows)
# Usage: just install-libretro "/path/to/RetroArch"
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

# List configured emulator accuracy test ROMs. ROM binaries stay out of git.
romtest-list:
    cargo run -p zeff-romtest -- list

# Validate emulator accuracy test manifests without running ROMs.
romtest-check:
    cargo run -p zeff-romtest -- check

# Populate the ignored canonical ROM cache from known local legacy paths.
romtest-prepare:
    cargo run -p zeff-romtest -- prepare

# Download verified public test ROM sources and extract selected clone-friendly entries.
romtest-fetch:
    cargo run -p zeff-romtest -- fetch --exclude-tier local

# Build the mGBA GBA test suite into the ignored ROM cache. Requires Docker Desktop.
[windows]
romtest-build-mgba-suite:
    powershell -ExecutionPolicy Bypass -File scripts/build-mgba-suite.ps1

# Build the asiekierka WonderSwan test suite into the ignored ROM cache.
# Requires Docker Desktop, or pass -Builder local from a Wonderful Toolchain shell.
[windows]
romtest-build-ws-suite:
    powershell -ExecutionPolicy Bypass -File scripts/build-ws-test-suite.ps1

# Build tiny generated Sega 8-bit smoke ROMs into the ignored ROM cache.
[windows]
romtest-build-sega8-smoke:
    powershell -ExecutionPolicy Bypass -File scripts/build-sega8-smoke-roms.ps1

# Build the generated PC Engine CD READ6/ADPCM/R15/IRQ fixture into the ignored cache.
[windows]
romtest-build-pce-cd-fixture:
    powershell -ExecutionPolicy Bypass -File scripts/build-pce-cd-adpcm-fixture.ps1

# Build the generated PC Engine VDC fetch-contention diagnostic.
[windows]
romtest-build-pce-vdc-contention-fixture:
    powershell -ExecutionPolicy Bypass -File scripts/build-pce-vdc-contention-fixture.ps1

# Validate the asiekierka WonderSwan test-suite build plan without downloading/building.
[windows]
romtest-build-ws-suite-dry-run:
    powershell -ExecutionPolicy Bypass -File scripts/build-ws-test-suite.ps1 -DryRun

# Build all source-only local test suites into the ignored ROM cache.
[windows]
romtest-build-local-suites:
    powershell -ExecutionPolicy Bypass -File scripts/build-mgba-suite.ps1
    powershell -ExecutionPolicy Bypass -File scripts/build-ws-test-suite.ps1
    powershell -ExecutionPolicy Bypass -File scripts/build-sega8-smoke-roms.ps1
    powershell -ExecutionPolicy Bypass -File scripts/build-pce-cd-adpcm-fixture.ps1
    powershell -ExecutionPolicy Bypass -File scripts/build-pce-vdc-contention-fixture.ps1

# Download local-only test ROM sources and extract them into the ignored cache.
# These may include unclear-license public test collections and stay out of default CI.
romtest-fetch-local:
    cargo run -p zeff-romtest -- fetch --tier local

# Download/fetch all Sega 8-bit test ROM artifacts, including source-backed local suites.
romtest-fetch-sega8:
    cargo run -p zeff-romtest -- fetch --tag sega8 --allow-missing

# Run fast emulator accuracy smoke tests from local ignored ROM cache.
romtest-smoke:
    cargo run -p zeff-romtest -- run --tier smoke --report-json rom-tests/results/smoke.json --report-md rom-tests/results/smoke.md --report-junit rom-tests/results/smoke.junit.xml --report-baseline rom-tests/results/smoke.baseline.json

# Run clone-friendly configured emulator accuracy tests. Local-only ROMs are excluded.
romtest-run:
    cargo run -p zeff-romtest -- run --exclude-tier local --report-json rom-tests/results/current.json --report-md rom-tests/results/current.md --report-junit rom-tests/results/current.junit.xml --report-baseline rom-tests/results/current.baseline.json

# Regenerate the committed clone-friendly ROM test baseline. Review this diff before committing.
romtest-baseline:
    cargo run -p zeff-romtest -- run --exclude-tier local --report-json rom-tests/results/current.json --report-md rom-tests/results/current.md --report-junit rom-tests/results/current.junit.xml --report-baseline rom-tests/baselines/current.json

# Run local-only ROM tests from ignored local caches.
romtest-run-local:
    cargo run -p zeff-romtest -- run --tier local --report-json rom-tests/results/local.json --report-md rom-tests/results/local.md --report-junit rom-tests/results/local.junit.xml --report-baseline rom-tests/results/local.baseline.json

# Run local-only WonderSwan ROM tests. Missing optional local artifacts are skipped.
romtest-run-local-ws:
    cargo run -p zeff-romtest -- run --tier local --core ws --allow-missing --report-json rom-tests/results/local-ws.json --report-md rom-tests/results/local-ws.md --report-junit rom-tests/results/local-ws.junit.xml --report-baseline rom-tests/results/local-ws.baseline.json

# Run generated local Sega 8-bit ROM tests.
romtest-run-local-sega8:
    cargo run -p zeff-romtest -- run --tier local --tag sega8 --report-json rom-tests/results/local-sega8.json --report-md rom-tests/results/local-sega8.md --report-junit rom-tests/results/local-sega8.junit.xml --report-baseline rom-tests/results/local-sega8.baseline.json

romtest-report-mgba-suite:
    cargo run -p zeff-romtest -- run --manifest-dir rom-tests/manifests/test-roms/gba.toml --tier local --tag mgba-suite --allow-missing --report-json rom-tests/results/mgba-suite.json --report-md rom-tests/results/mgba-suite.md --report-junit rom-tests/results/mgba-suite.junit.xml --report-baseline rom-tests/results/mgba-suite.baseline.json

romtest-report-ws-suite:
    cargo run -p zeff-romtest -- run --manifest-dir rom-tests/manifests/test-roms/ws-asiekierka-local.toml --allow-missing --report-json rom-tests/results/ws-asiekierka-suite.json --report-md rom-tests/results/ws-asiekierka-suite.md --report-junit rom-tests/results/ws-asiekierka-suite.junit.xml --report-baseline rom-tests/results/ws-asiekierka-suite.baseline.json

romtest-report-sega8-generated:
    cargo run -p zeff-romtest -- run --manifest-dir rom-tests/manifests/test-roms/sega8-generated-local.toml --allow-missing --report-json rom-tests/results/sega8-generated.json --report-md rom-tests/results/sega8-generated.md --report-junit rom-tests/results/sega8-generated.junit.xml --report-baseline rom-tests/results/sega8-generated.baseline.json

romtest-report-pce-cd-fixture:
    cargo run -p zeff-romtest -- run --manifest-dir rom-tests/manifests/test-roms/pce-generated-local.toml --allow-missing --report-json rom-tests/results/pce-cd-fixture.json --report-md rom-tests/results/pce-cd-fixture.md --report-junit rom-tests/results/pce-cd-fixture.junit.xml --report-baseline rom-tests/results/pce-cd-fixture.baseline.json

# Run all Sega 8-bit ROM tests, including source-backed accuracy and local suites.
romtest-run-sega8:
    cargo run -p zeff-romtest -- run --tag sega8 --report-json rom-tests/results/sega8.json --report-md rom-tests/results/sega8.md --report-junit rom-tests/results/sega8.junit.xml --report-baseline rom-tests/results/sega8.baseline.json

# Regenerate the committed local-only ROM test baseline. Requires local ROM cache.
romtest-baseline-local:
    cargo run -p zeff-romtest -- run --tier local --report-json rom-tests/results/local.json --report-md rom-tests/results/local.md --report-junit rom-tests/results/local.junit.xml --report-baseline rom-tests/baselines/local.json

# Compare a run JSON report against a curated baseline.
romtest-compare baseline="rom-tests/baselines/current.json" actual="rom-tests/results/current.json":
    cargo run -p zeff-romtest -- compare --baseline "{{baseline}}" --actual-json "{{actual}}"

# Print status/coverage/suite tables from the committed ROM test baseline.
romtest-status baseline="rom-tests/baselines/current.json":
    cargo run -p zeff-romtest -- status --baseline "{{baseline}}"

# Print status/coverage/suite tables from the committed local-only ROM test baseline.
romtest-status-local baseline="rom-tests/baselines/local.json":
    cargo run -p zeff-romtest -- status --baseline "{{baseline}}"

# List local user-owned compatibility game entries. Real local*.toml manifests are ignored.
romtest-list-compat:
    cargo run -p zeff-romtest -- list --tier compat --include-games

# Generate an ignored local compatibility manifest from a folder of user-owned ROM dumps.
romtest-generate-compat rom_dir output="rom-tests/manifests/compat-games/local-generated.toml" frames="3600":
    cargo run -p zeff-romtest -- generate-compat --rom-dir "{{rom_dir}}" --output "{{output}}" --max-frames {{frames}}

# Generate an ignored local compatibility manifest for a specific core. Use this for zipped ROM folders.
romtest-generate-compat-core rom_dir core output="rom-tests/manifests/compat-games/local-generated.toml" frames="3600":
    cargo run -p zeff-romtest -- generate-compat --core "{{core}}" --rom-dir "{{rom_dir}}" --output "{{output}}" --max-frames {{frames}}

# Run ignored local user-owned compatibility game entries. Missing paths are reported as skipped.
romtest-run-compat:
    cargo run -p zeff-romtest -- run --tier compat --include-games --allow-missing --report-json rom-tests/results/compat.json --report-md rom-tests/results/compat.md --report-junit rom-tests/results/compat.junit.xml --report-baseline rom-tests/results/compat.baseline.json

# Run ignored local user-owned Sega 8-bit compatibility game entries generated with the sega8 tag.
romtest-run-compat-sega8:
    cargo run -p zeff-romtest -- run --tier compat --tag sega8 --include-games --allow-missing --report-json rom-tests/results/compat-sega8.json --report-md rom-tests/results/compat-sega8.md --report-junit rom-tests/results/compat-sega8.junit.xml --report-baseline rom-tests/results/compat-sega8.baseline.json

# Run compatibility game entries through a prebuilt emulator exe. Useful while live-control is open.
romtest-run-compat-exe exe="target/debug/zeff-boy.exe":
    cargo run -p zeff-romtest -- run --tier compat --include-games --allow-missing --zeff-boy "{{exe}}" --report-json rom-tests/results/compat.json --report-md rom-tests/results/compat.md --report-junit rom-tests/results/compat.junit.xml --report-baseline rom-tests/results/compat.baseline.json

# Print status/coverage/suite tables from the latest local compatibility run.
romtest-status-compat:
    cargo run -p zeff-romtest -- status --actual-json rom-tests/results/compat.json

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

# CI follows the latest stable Rust release.
sync-ci-toolchain:
    rustup update stable

[windows]
ci-tools:
    if (-not (Get-Command cargo-nextest -ErrorAction SilentlyContinue)) { cargo install cargo-nextest --locked }
    if (-not (Get-Command cargo-deny -ErrorAction SilentlyContinue)) { cargo install cargo-deny --locked }

[unix]
ci-tools:
    command -v cargo-nextest >/dev/null || cargo install cargo-nextest --locked
    command -v cargo-deny >/dev/null || cargo install cargo-deny --locked

# Check that no native-only APIs leaked into shared code
[unix]
lint-platform-leaks:
    ! grep -rn --include='*.rs' -E '(rfd::|gilrs::|cpal::|dirs::|open::that|ureq::|pollster::block_on|nokhwa::)' src/ --exclude-dir=platform --exclude-dir=input --exclude-dir=audio --exclude='cli/*' | grep -v '// platform-ok'

[windows]
lint-platform-leaks:
    $hits = Get-ChildItem -Path src -Recurse -Filter *.rs | Where-Object { $_.FullName -notmatch '\\(platform|input\\native|audio\\native|audio\\tests|camera|cli|mods\\native|libretro_common)' -and $_.Name -ne 'native.rs' } | Select-String -Pattern 'rfd::|gilrs::|cpal::|dirs::|open::that|ureq::|pollster::block_on|nokhwa::' | Where-Object { $_.Line -notmatch '// platform-ok' }; if ($hits) { $hits; exit 1 } else { Write-Host 'No platform leaks found.' }

# Run full CI pipeline locally (fmt + lint + platform check + test + deny)
ci-local: sync-ci-toolchain ci-tools fmt-check lint-all lint-platform-leaks test-all deny

# Run WASM CI check locally (requires wasm32 target and Trunk)
ci-local-wasm: sync-ci-toolchain lint-wasm check-wasm build-wasm-ghpages

# Check that fuzz targets compile (requires nightly)
fuzz-check:
    cargo +nightly check --manifest-path fuzz/Cargo.toml

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

[windows]
get-all-code-crates:
    $allCode = ""; Get-ChildItem -Path crates -Recurse -Filter *.rs | Sort-Object FullName | ForEach-Object { $allCode += "`n// ===== $($_.FullName) =====`n"; $allCode += Get-Content $_.FullName -Raw }; Set-Clipboard -Value $allCode

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

# Profile all cores, or one of: gb, gba, nes, pce, sega8, ws
profile-cores core="all" frames="3000":
    $env:ZEFF_PROFILE_CORE = "{{core}}"; $env:ZEFF_PROFILE_FRAMES = "{{frames}}"; cargo run --profile profiling --bin profile_cores --features profile-cores

# Flamegraph all cores, or one selected core (requires admin on Windows)
flamegraph-cores core="all" frames="3000":
    $env:ZEFF_PROFILE_CORE = "{{core}}"; $env:ZEFF_PROFILE_FRAMES = "{{frames}}"; cargo flamegraph --profile profiling --bin profile_cores --features profile-cores -o "flamegraph-{{core}}.svg"

# Train synthetic cores plus available local gameplay and build a PGO release
[windows]
build-pgo frames="1200":
    powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-pgo.ps1 -Frames {{frames}}

# List the deterministic local PGO corpus without compiling or training.
[windows]
pgo-corpus:
    powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-pgo.ps1 -ListGameplayCorpus

# Run one core's Criterion suite: gb, gba, nes, pce, sega8, or ws
bench core:
    cargo bench --bench "{{core}}_benchmarks" -p "zeff-{{core}}-core"

# Run all Criterion benchmarks
bench-all:
    cargo bench --workspace

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

