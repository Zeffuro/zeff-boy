# Zeff-Boy Accuracy & Subsystem Status

**Last updated:** 2026-03-27

This document tracks the accuracy status of each hardware subsystem in both the Game Boy (GB/GBC) and NES emulation cores.

## Legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Implemented and hardware-accurate (or close to it) |
| ⚠️ | Implemented but with known approximations |
| ❌ | Not implemented / stub |
| 🧪 | Has dedicated unit/integration tests |

---

## Game Boy / Game Boy Color (`zeff-gb-core`)

### CPU

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| All official opcodes (256 + 256 CB) | ✅ | 🧪 | Decoded dispatch with `read_reg`/`write_reg` helpers |
| Flag behavior (Z, N, H, C) | ✅ | 🧪 | Tested per opcode group |
| HALT instruction | ✅ | 🧪 | Includes HALT bug (PC fails to increment when IF & IE with IME=0) |
| STOP instruction | ✅ | | CGB speed switch triggered via STOP |
| IME / EI delay | ✅ | | EI enables interrupts after the *next* instruction (PendingEnable state) |
| Interrupt dispatch | ✅ | | Priority: VBlank > LCD STAT > Timer > Serial > Joypad |
| CGB double speed mode | ✅ | 🧪 | KEY1 register, 3 tests |

### PPU

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| Background rendering | ✅ | 🧪 | Correct tile data / tile map addressing |
| Window rendering | ✅ | 🧪 | Window line counter, enable/disable mid-frame |
| Sprite rendering (DMG) | ✅ | | 10-sprite-per-line limit, X-then-OAM priority |
| Sprite rendering (CGB) | ✅ | | OAM-index-only priority, tile attributes |
| Mode transitions (OAM/Draw/HBlank/VBlank) | ✅ | 🧪 | |
| Variable Mode 3 (pixel transfer) duration | ✅ | 🧪 | Based on SCX, sprite count, window penalty; 4 tests |
| STAT interrupt sources | ✅ | | LYC=LY, OAM, VBlank, HBlank |
| OAM corruption (write during Mode 2) | ✅ | | `maybe_trigger_oam_corruption` with fast-path early exits |
| CGB VRAM banking | ✅ | 🧪 | Bank 0/1 switching |
| CGB palettes (BCPS/BCPD/OCPS/OCPD) | ✅ | 🧪 | Auto-increment, mode 3 blocking, 8+ tests |
| CGB tile attributes | ✅ | | Flip, palette, VRAM bank, BG priority |
| Color correction (LCD simulation) | ✅ | 🧪 | None / GBC LCD / Custom modes, 5 tests |
| SGB border/palettes | ⚠️ | | Basic SGB support present |

### Timer

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| DIV register (16-bit system counter) | ✅ | 🧪 | |
| TIMA increment (TAC frequency select) | ✅ | 🧪 | |
| TMA reload delay | ✅ | 🧪 | 4 T-cycle delay after TIMA overflow (per Pan Docs) |
| Timer interrupt delay | ✅ | 🧪 | Raised with TMA reload, not on overflow itself |
| TAC falling-edge glitch | ✅ | 🧪 | Switching TAC frequency or disabling can trigger extra TIMA increment |
| DIV-reset during pending overflow | ✅ | 🧪 | |
| TMA write during delay period | ✅ | 🧪 | |
| Optimized stepping | ✅ | | Precomputed mask/enabled flag outside per-cycle loop |

**Total timer tests: 15** (11 original + 4 edge cases)

### APU (Audio)

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| Pulse channels 1 & 2 | ✅ | | Duty cycle, envelope, sweep (ch1 only) |
| Wave channel | ✅ | | 4-bit samples, bank switching (CGB) |
| Noise channel | ✅ | | LFSR with 7-bit/15-bit modes |
| Channel muting (per channel) | ✅ | | `[bool; 4]`:Pulse1, Pulse2, Wave, Noise |
| Sample generation gating | ✅ | | `sample_generation_enabled` skips mixing when globally muted |
| Frame sequencer | ✅ | | Drives length counters, envelopes, sweeps |

### Cartridge / Mappers

| Mapper | Status | Tests | Notes |
|--------|--------|-------|-------|
| ROM Only | ✅ | | No banking |
| MBC1 | ✅ | 🧪 | 14 tests: bank switching, 0→1 correction, 5-bit mask, RAM enable, modes, wrapping, save state |
| MBC2 | ✅ | 🧪 | 9 tests: 4-bit RAM, bit8 address, bank switching, 512B mirror, save state |
| MBC3 | ✅ | 🧪 | 7 tests: bank switching, RAM banking, RTC latch/read |
| MBC5 | ✅ | 🧪 | 11 tests: 9-bit bank, bank 0 valid, RAM switching, rumble bit, save state |
| MBC7 | ✅ | 🧪 | 5 tests: accelerometer, EEPROM |
| HuC1 | ✅ | | Infrared LED support |
| HuC3 | ✅ | | Mapper + RTC |

### Serial

| Feature | Status | Notes |
|---------|--------|-------|
| Internal clock serial | ✅ | Byte transfer with shift register |
| External clock serial | ⚠️ | No link cable support:local-only |

### Joypad

| Feature | Status | Notes |
|---------|--------|-------|
| Button/direction matrix | ✅ | JOYP register read/write |
| Joypad interrupt | ✅ | Triggered on button press |

### Save States

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| Format with magic + version | ✅ | | `ZBSTATE\0` + format version |
| Full machine state round-trip | ✅ | 🧪 | Timer save state roundtrip tested |
| BESS export/import | ✅ | | Cross-emulator compatibility |
| ROM hash validation | ✅ | | Prevents cross-ROM state loading |

### Debug Features

| Feature | Status | Notes |
|---------|--------|-------|
| Breakpoints | ✅ | Address-based PC breakpoints |
| Watchpoints | ✅ | Memory read/write watchpoints |
| Step / Continue | ✅ | Single-step and resume |
| Opcode log (ring buffer) | ✅ | 32-entry ring buffer, toggled by debug UI |
| Rewind | ✅ | System-agnostic RewindBuffer |
| Replay | ✅ | System-agnostic ReplayRecorder/Player |

---

## NES (`zeff-nes-core`)

### CPU (6502)

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| All official opcodes | ✅ | 🧪 | 42 unit tests + 2 nestest integration tests |
| All stable unofficial opcodes | ✅ | 🧪 | LAX, SAX, DCP, ISB, SLO, RLA, SRE, RRA, ANC, ALR, ARR, AXS, NOP variants, KIL/JAM |
| Unstable unofficial opcodes | ⚠️ | | ANE, SHA, SHX, SHY, TAS, LAS:log warning only |
| Page-crossing cycle penalties | ✅ | 🧪 | `execute_opcode()` returns extra cycles; tested |
| Branch taken/not-taken cycles | ✅ | 🧪 | 2/3/4 cycles for not-taken/taken/taken+page-cross |
| BRK/NMI hijack | ✅ | 🧪 | NMI vector used when NMI pending during BRK vector fetch |
| JMP indirect page boundary bug | ✅ | 🧪 | `$02FF` wraps to `$0200`, not `$0300` |
| Zero-page X wrapping | ✅ | 🧪 | Wraps within zero page |
| Addressing modes (13 total) | ✅ | 🧪 | Including `addr_immediate` with `&Bus` (not `&mut`) |
| `nestest.nes` (official opcodes) | ✅ | 🧪 | Integration test validates against expected result |
| `nestest.nes` (unofficial opcodes) | ✅ | 🧪 | Integration test validates against expected result |

### PPU

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| Background rendering | ✅ | | 341-dot per-scanline rendering |
| Sprite rendering | ✅ | | 8 sprites per scanline, priority |
| Sprite #0 hit | ✅ | | x=255 exclusion, rendering-disabled guard, left clipping |
| Sprite overflow flag | ✅ | | **Hardware bug emulated**: incorrect `n*4 + m` indexing with `m = (m+1) & 3` on miss |
| Greyscale | ✅ | | Masks palette index `& 0x30` before color lookup |
| Color emphasis (bits 5-7 of $2001) | ✅ | | Dims non-emphasized RGB channels after greyscale |
| Open bus / IO latch | ✅ | | PPU data bus latch tracked and returned for unused registers |
| Fine scrolling (X and Y) | ✅ | | `v` and `t` registers, fine_x |
| Nametable mirroring | ✅ | | Horizontal, vertical, single-screen, four-screen |
| Sprite eval timing | ⚠️ | | Executes at dot 0 instead of dots 65-256; games altering OAM mid-scanline may glitch |
| Per-dot rendering | ✅ | | `compose_pixel()` evaluates BG+sprite overlap every dot |
| Catch-up PPU scheduling | ❌ | | PPU ticks per CPU cycle, not on-demand; correct but slower than possible |

### APU

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| Pulse channels 1 & 2 | ✅ | | Duty cycle, envelope, sweep, length counter |
| Triangle channel | ✅ | | Linear counter, outputs last sequence value when disabled (no pop) |
| Noise channel | ✅ | | LFSR with short/long modes, envelope, length counter |
| DMC channel | ✅ | | DMA-based sample fetching through bus, output level register |
| Non-linear mixing | ✅ | | Hardware lookup formulas (`95.88 / (8128.0 / pulse_sum + 100.0)`, etc.) |
| Frame sequencer | ✅ | | 4-step and 5-step modes, drives length/envelope/sweep |
| Frame counter write ($4017) | ✅ | | Odd-cycle offset, immediate clock if 5-step mode |
| Channel muting (per channel) | ✅ | | `[bool; 5]`:Pulse1, Pulse2, Triangle, Noise, DMC |
| Sample generation gating | ✅ | | `sample_generation_enabled` flag |
| DMC DMA stall (parity-aware) | ✅ | | 3 cycles even / 4 cycles odd / +1 if OAM DMA conflict |
| Debug sample collection | ✅ | | Gated by `debug_collection_enabled`, toggled by APU viewer |
| Status register peek | ✅ | | `peek_status()`:non-mutating, doesn't clear frame_irq |

### Bus / Memory Map

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| CPU memory map ($0000-$FFFF) | ✅ | | RAM mirror, PPU mirror, APU/IO, cartridge |
| Open bus | ✅ | | `cpu_open_bus` tracks last bus value; write-only regs return it |
| `cpu_peek()` (non-mutating debug read) | ✅ | | No PPU/controller side effects |
| OAM DMA | ✅ | | 513/514 cycles (even/odd alignment) |
| OAM DMA stall properly consumed | ✅ | | `dma_stall_cycles` consumed in `runtime.rs` |
| Controller 1 & 2 | ✅ | | Strobe latch, 8-bit shift register for both controllers |

### Cartridge / Mappers

| Mapper | iNES # | Status | Tests | Notes |
|--------|--------|--------|-------|-------|
| NROM | 0 | ✅ | | No banking |
| SxROM / MMC1 | 1 | ✅ | | PRG/CHR banking, shift register |
| UxROM | 2 | ✅ | | PRG banking, bus conflicts |
| CNROM | 3 | ✅ | | CHR banking |
| TxROM / MMC3 | 4 | ✅ | | IRQ scanline counter, PRG/CHR banking |
| ExROM / MMC5 | 5 | ⚠️ | | Basic implementation; scanline detection uses PPU dot 260/324 instead of nametable-read monitoring |
| AxROM | 7 | ✅ | | Single-screen mirroring select |
| Bandai FCG-16 | 16 | ✅ | | EPROM, CHR banking, IRQ |
| VRC4 | 21/23/25 | ✅ | | PRG/CHR banking, IRQ, address line remapping |
| FME-7 / Sunsoft 5B | 69 | ✅ | | PRG/CHR/WRAM banking, IRQ |
| Action 52 | 228 | ✅ | | Multicart |

All mapper `chr_read()` methods guard against empty CHR with `is_empty()` check.

### Header Parsing

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| iNES 1.0 | ✅ | | PRG/CHR sizes, mapper ID, mirroring, battery, trainer |
| NES 2.0 | ✅ | | Exponent sizes, submapper, timing modes, extended mapper IDs |
| Junk-tail detection | ✅ | | Handles dirty ROMs with trailing garbage |
| Zero PRG ROM rejection | ✅ | | Returns descriptive error |
| Descriptive error messages | ✅ | | Bad magic, truncated files, unsupported mappers |

### Cheats

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| Game Genie (6-letter) | ✅ | 🧪 | 10 tests: decode, intercept, case-insensitive, dashes |
| Game Genie (8-letter + compare) | ✅ | 🧪 | Compare byte match/mismatch tested |
| ROM interception ($8000-$FFFF) | ✅ | | Wired into `Bus::cpu_read()` |
| RAM cheats (direct write) | ✅ | | Applied via `cpu_write()` each frame |

### Save States

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| Format v2 (lz4 compressed) | ✅ | 🧪 | `ZBNSTATE` magic + version 2 |
| Format v1 (raw, backward compat) | ✅ | 🧪 | Auto-detected on load |
| CPU state roundtrip | ✅ | 🧪 | |
| Bus state roundtrip | ✅ | 🧪 | |
| ROM hash validation | ✅ | 🧪 | Hash mismatch rejection tested |
| Truncated/corrupt rejection | ✅ | 🧪 | Bad magic, bad version, truncated data tested |
| Bounds masking on deserialize | ✅ | | `sequence_pos & 31`, `duty & 3`, `rate_index & 0x0F` |

### Debug Features

| Feature | Status | Notes |
|---------|--------|-------|
| Breakpoints | ✅ | Address-based PC breakpoints |
| Watchpoints | ✅ | Memory read/write via `DebugTraceEvent` bus trace |
| Step / Continue | ✅ | Single-step and resume |
| Opcode log (ring buffer) | ✅ | 32-entry ring buffer, toggled by debug UI |
| Disassembler (all 256 opcodes) | ✅ | Unofficial opcodes prefixed with `*` |
| Rewind | ✅ | System-agnostic RewindBuffer via `EmuBackend` |
| Replay | ✅ | System-agnostic ReplayRecorder/Player |

---

## Known Approximations & Future Work

### NES
- **Sprite eval timing**: Evaluates all sprites at dot 0 instead of spreading over dots 65-256. Most games unaffected; mid-scanline OAM changes may not render correctly.
- **MMC5 scanline detection**: Uses PPU dot 260/324 instead of monitoring nametable reads. Castlevania III split-screen effects may not be fully accurate.
- **Catch-up PPU**: PPU is ticked every CPU cycle rather than on-demand. Correct behavior but suboptimal performance. A catch-up scheduler could advance PPU in batches.
- **Unstable unofficial opcodes**: ANE ($8B), SHA ($9F/$93), SHX ($9E), SHY ($9C), TAS ($9B), LAS ($BB) log a warning but produce approximate results.

### Game Boy
- **SGB support**: Basic:border and palette commands, but full SGB feature set not verified.
- **Serial link cable**: Internal clock only; no external/multiplayer link support.

### Both
- **Save state format**: Manual binary serialization. Adding/removing fields requires careful version management. A serde-based approach would be more maintainable but risks backward compatibility.
- **No automated test ROM validation**: Blargg, mooneye-gb, and other test ROM suites are not distributed with the repository. Manual testing is recommended for accuracy verification.

