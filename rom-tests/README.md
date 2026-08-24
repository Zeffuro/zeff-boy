# ROM tests

Metadata for test ROMs lives here. The ROMs themselves do not.

```powershell
just romtest-status
just romtest-fetch
just romtest-smoke
just romtest-run
just romtest-compare

# local-only ROM cache
just romtest-run-local
just romtest-status-local

# generated local Sega 8-bit smoke ROMs
just romtest-build-sega8-smoke
just romtest-build-pce-cd-fixture
just romtest-fetch-sega8
just romtest-run-sega8
just romtest-run-local-sega8

# local user-owned compatibility games
just romtest-list-compat
just romtest-run-compat
just romtest-run-compat-sega8
just romtest-status-compat
```

`manifests/test-roms` is for public test ROMs.
`manifests/compat-games` is for local game compatibility notes. Copy `example.toml` to an ignored `local*.toml` file before adding real game paths or names.
`cache` and `results` are ignored.

Some local-tier suites are source-only. Build them into the ignored cache before running the
corresponding local tests:

```powershell
just romtest-build-ws-suite
just romtest-run-local-ws
just romtest-build-sega8-smoke
just romtest-build-pce-cd-fixture
just romtest-run-local-sega8
```

`scripts/build-ws-test-suite.ps1` downloads the pinned MIT-licensed
`asiekierka/ws-test-suite` source archive, verifies its SHA-256, builds upstream's default
`TARGET=wswan/small`, and copies generated `.ws`/`.wsc` files from `build/roms/*` into
`rom-tests/cache/ws/asiekierka/ws-test-suite/`. It uses Docker by default when Docker is available;
run it from a Wonderful Toolchain shell with `-Builder local` if you prefer a local toolchain.
Generated ROMs remain out of git. If you redistribute them elsewhere, include the upstream MIT
license notice copied into the cache as `LICENSE.ws-test-suite.txt`.

`scripts/build-pce-cd-adpcm-fixture.ps1` generates a redistributable mini System Card and
Mode1/2048 CUE set under `rom-tests/cache/pce/generated/cd-adpcm-irq/`. The card uses the ordinary
public PCE-CD loader and register interface: it sends a 17-sector READ6, enables CD-to-ADPCM DMA,
checks the first and last transferred bytes, then records live IRQ2 half/end events and broad
cycle-derived timing windows. It publishes `ZPCE` plus status/counters in work RAM; the
`pce_memory_status` romtest pass kind maps that record to the public `--expect-test-pass` contract.
Build it before `just romtest-run-local`.

`scripts/build-sega8-smoke-roms.ps1` generates tiny local Sega 8-bit smoke ROMs into
`rom-tests/cache/sega8/generated/`. These ROMs are generated from tracked source, remain out of git,
and are covered by `rom-tests/manifests/test-roms/sega8-generated-local.toml`. They currently
exercise SMS Mode 4 priority over sprites, Game Gear Mode 4 priority inside the cropped viewport
with GG CRAM, SG-1000 TMS9918 Graphics I rendering, and Codemasters mapper detection/bank
switching.

`rom-tests/manifests/test-roms/sega8-source-backed.toml` adds pinned public source-backed Sega
8-bit tests:

- ZEXALL-SMS 0.20 (`zexall.sms` and `zexdoc.sms`) from a GPLv2 source/binary ZIP, using SDSC
  debug-console text as a boot/output assertion.
- SMS VDP Test 1.31 and 64 Color Palette Test 1.00 as local-only boot checks because their public
  ZIPs include source but no clear top-level license.
- JoppyFurr SN76489 TestRom 1.00 for SMS, Game Gear, and SG-1000 as local-only audio checks. These
  use scripted input and assert nonzero PSG output.

Prepare and run the full Sega 8-bit set with:

```powershell
just romtest-build-sega8-smoke
just romtest-fetch-sega8
just romtest-run-sega8
```

For Sega 8-bit bring-up, `scripts/scan-sega8-romset.ps1` can scan a local user-owned SMS/GG/SG
ROM set recursively and print only aggregate header/hint counts:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/scan-sega8-romset.ps1 -RomRoot Z:\Android\Roms
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/scan-sega8-romset.ps1 -Extensions sms -Limit 40
```

The scanner intentionally omits ROM names, paths, and hashes, and it does not write compatibility
manifests. Use `-Limit N` for bounded scans on large collections. Use it for loader coverage checks,
not for committed game reports.

For aggregate run-quality probes, skip the metadata pass and run frames through the core:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/scan-sega8-romset.ps1 -Probe -ProbeOnly -Extensions gg -Limit 120 -ProbeFrames 180
```

This prints only aggregate CPU-trap, mapper, framebuffer, VDP, and PSG counts. It is useful for
checking whether private ROM samples actually advance frames and render plausible output without
leaking names or paths.

To find likely game issues without printing ROM names in the terminal, write a local TSV report:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/scan-sega8-romset.ps1 -Probe -ProbeOnly -Extensions sms -Limit 200 -ProbeFrames 180 -IssueReport rom-tests/results/sega8-issues.tsv
```

The report defaults to redacted `location = hidden` rows with issue IDs, systems, reasons, mapper,
trap, and frame/video summaries. Add `-IssueReportPaths` only when you explicitly want local ROM
paths written into the ignored report file.

For user-owned Sega 8-bit game compatibility manifests, use the existing compatibility generator:

```powershell
just romtest-generate-compat Z:\Android\Roms rom-tests/manifests/compat-games/local-sega8.toml 3600
just romtest-run-compat-sega8
```

Generated `.sms`, `.gg`, `.sg`, and `.sc` entries include a `sega8` tag so they can be run as a
family without mixing them with unrelated local compatibility entries.

Do not commit commercial games or random ROM dumps.
