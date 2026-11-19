# Mega Drive diagnostics

ROMs and generated data stay in the ignored cache. Pins, licenses and file hashes
are recorded in `md-diagnostics.json` and `sources.toml`.

```powershell
python -B scripts/fetch-m68k-vectors.py --bus-contracts --nominal-cycles
cargo run -p zeff-m68k-verify --bin verify-vectors -- rom-tests/cache/m68k/nominal-bus-v1.json
python -B scripts/fetch-m68k-vectors.py --phase-export --cohorts MOVE.b MOVE.w MOVE.l MOVEA.w MOVEA.l --output rom-tests/cache/m68k/move-phases-v1.jsonl
cargo run -p zeff-m68k-verify --bin verify-phases -- rom-tests/cache/m68k/move-phases-v1.jsonl
cargo run -p zeff-m68k-verify --bin verify-address-faults -- rom-tests/cache/m68k/move-phases-v1.jsonl
python -B scripts/fetch-md-diagnostics.py --build-bcd
cargo run -p zeff-m68k-verify --bin verify-bcd -- rom-tests/cache/md-diagnostics/bcd-source/data/bcd-table.bin
cargo run -p zeff-m68k-verify --bin verify-bcd -- rom-tests/cache/md-diagnostics/bcd-source/data/bcd-table.bin --timed-memory
cargo run -p zeff-romtest -- run --core md --tier local --zeff-boy target/release/zeff-boy.exe
```

The BCD build uses Windows, local `g++`, and a hash-pinned AS assembler. Downloading
without `--build-bcd` obtains the 240p ROMs and BCD sources. Original source and
license notices accompany the cached diagnostics; no commercial ROMs are included.

| Diagnostic | Evidence and scope |
| --- | --- |
| [SingleStepTests/m68000](https://github.com/SingleStepTests/m68000/tree/64b253116a3de04aaac4346c43680960dc9b67e5) | MIT. Complete selected cohorts, with counted exclusions. MAME-generated reference records. Architectural state and nominal totals; the separate phase verifier checks admitted prefetch/ordered whole-transfer/idle routes. Trace records stop before exception entry. A separate fault validator checks selected address-error frames; these are reference results, with no fresh hardware or pin-phase proof. |
| [Flamewing BCD verifier](https://github.com/flamewing/68k-bcd-verifier/tree/39a01be528b0744302bf1dc9b3463fc22a3fc45f) | GPL-3.0-or-later. Exhaustive ABCD/SBCD/NBCD, including invalid digits and X/Z combinations. Author reports Model 1 and Model 3 VA2 hardware passes. Locally built ROM and table; no fresh console run. |
| [240p Test Suite Genesis 1.32](https://artemiourbina.itch.io/240p-test-suite) | GPLv2 notice included. Official published ROM: visual patterns, scrolling, controller and sound checks. Use the regular ROM; `_emu` enables Sega CD identification. Console execution remains pending. |
| [Nemesis VDP FIFO diagnostics](https://www.exodusemulator.com/about/news/29-exodus-2-0-released-project-now-open-source) | Candidate for FIFO/status/access timing. License and exact artifact admission pending; not bundled. |

No single ROM proves every hardware function. Each implementation needs a named
witness, expected result, hardware/model provenance, boundary cases and a bounded
failure signal. Reference-derived and synthetic results are recorded separately
from physical measurements. The following coverage is required before admission:

| Area | Required witnesses beyond ordinary game boot |
| --- | --- |
| 68000 ISA | Every legal instruction, operand size/addressing mode, aliases, flags and variable timing; hardware-backed BCD and independently pinned vectors. |
| CPU exception/timing | Trace/interrupt priority and masks, STOP wake, bus/address frames, partial transfers, reset signal, TAS bus ownership, MOVEM discard reads, prefetch, self-modifying code and waits. |
| Machine bus/Z80 | Mapping/mirrors/byte lanes/open bus, arbitration/reset/BUSREQ/bank window, two-CPU communication and interrupt ordering. |
| VDP memory/ports | Register masks, command latch, VRAM/CRAM/VSRAM, status side effects, FIFO capacity/order, DMA transfer/fill/copy and CPU stalls. |
| VDP display | Planes/window, priorities, sprites/collision/limits, scroll modes, H32/H40, shadow/highlight, interlace, blanking, counters and H/V interrupt boundaries. |
| Audio | PSG tones/noise/variant behavior; OPN2 operators/envelopes/algorithms/LFO/timers/DAC/SSG-EG/channel modes and busy timing. Hardware recordings and chip-model provenance; cadence/chunk/state continuation checks. |
| Inputs/regions/media | Three/six-button TH sequences and timeout, NTSC/PAL/Japan timing, reset policy, linear cartridges, SRAM/EEPROM and individually admitted boards. |
| Zeff integration | Native/WASM/OS execution, loading/archive identification, audio/video/input, transactional states/persistence, debugger peeks, replay/TAS, rewind/run-ahead and separately gated netplay. |

The standard ROM runner checks published 240p NTSC/PAL menus and gray ramps.
The ramps exercise plane-priority shadow and normal intensity. These are output
regressions; game compatibility and advanced capabilities need their
own witnesses.
