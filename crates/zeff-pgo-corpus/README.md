# Zeff PGO corpus

This crate generates small, deterministic emulator inputs owned by this
repository. It does not download, embed, or redistribute game ROMs, BIOS dumps,
or third-party test suites.

The normal corpus covers GB, GBC, GBA, NES, PC Engine, SMS, Game Gear,
WonderSwan, and WonderSwan Color. Each output is intended for headless training
of the instrumented release executable. The manifest has relative paths and
SHA-256 identities so a consumer can verify what it executes.

GBA inputs initialize Mode 0 text backgrounds, visible sprites, palette/tile
data, a timer and PSG audio entirely in guest code. ARM and Thumb loops mix
arithmetic, shifts, conditions and bounded EWRAM/IWRAM transfers; a third
variant copies its ARM loop into IWRAM before execution. Runtime tests check
the hot PCs and actual bus addresses, in addition to visible pixels and audio.
These are representative synthetic workloads, not performance guarantees for
games; qualify optimized builds on separate holdouts.

ColecoVision is intentionally separate. `coleco_fixture()` returns a cartridge
and a synthetic test BIOS for an explicit test-only loader route. The synthetic
BIOS is not a recognized retail firmware image and must never weaken the normal
firmware policy.
