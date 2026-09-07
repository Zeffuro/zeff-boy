//! Deterministic, owned inputs for release-binary PGO training.
//!
//! No generated output is tracked. Call [`write_fresh_corpus`] to create a
//! caller-selected fresh directory and its self-describing manifest.

use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

mod gba;
use gba::{
    arm_active_rom as gba_arm_active_rom, memory_rom as gba_memory_rom,
    thumb_active_rom as gba_thumb_active_rom,
};
mod ws;
use ws::rom as ws_rom;

pub const SUGGESTED_FRAMES: u32 = 600;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    pub id: &'static str,
    pub system: &'static str,
    pub extension: &'static str,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CorpusManifest {
    pub schema: u32,
    pub fixtures: Vec<ManifestFixture>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ManifestFixture {
    pub id: String,
    pub system: String,
    /// Filename relative to the manifest directory.
    pub path: String,
    pub sha256: String,
    pub frames: u32,
}

/// Normal release-corpus inputs. Coleco is deliberately separate because the
/// shipping loader only accepts recognized retail firmware.
pub fn fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            id: "gb-ppu-apu",
            system: "GB",
            extension: "gb",
            bytes: gb_rom(false),
        },
        Fixture {
            id: "gbc-ppu-apu",
            system: "GBC",
            extension: "gbc",
            bytes: gb_rom(true),
        },
        Fixture {
            id: "gba-arm-active",
            system: "GBA",
            extension: "gba",
            bytes: gba_arm_active_rom(),
        },
        Fixture {
            id: "gba-thumb-active",
            system: "GBA",
            extension: "gba",
            bytes: gba_thumb_active_rom(),
        },
        Fixture {
            id: "gba-arm-memory",
            system: "GBA",
            extension: "gba",
            bytes: gba_memory_rom(),
        },
        Fixture {
            id: "nes-ppu-apu",
            system: "NES",
            extension: "nes",
            bytes: nes_rom(),
        },
        Fixture {
            id: "pce-video-loop",
            system: "PCE",
            extension: "pce",
            bytes: pce_rom(false),
        },
        Fixture {
            id: "pce-memory-loop",
            system: "PCE",
            extension: "pce",
            bytes: pce_rom(true),
        },
        Fixture {
            id: "sms-vdp-psg",
            system: "SMS",
            extension: "sms",
            bytes: sega8_rom(false),
        },
        Fixture {
            id: "gg-vdp-psg",
            system: "GG",
            extension: "gg",
            bytes: sega8_rom(true),
        },
        Fixture {
            id: "ws-mono-loop",
            system: "WS",
            extension: "ws",
            bytes: ws_rom(false),
        },
        Fixture {
            id: "wsc-color-loop",
            system: "WSC",
            extension: "wsc",
            bytes: ws_rom(true),
        },
    ]
}

/// Returns `(cartridge, synthetic_bios)`. The BIOS is a deterministic test
/// program, not a recognized retail ColecoVision firmware image.
pub fn coleco_fixture() -> (Vec<u8>, Vec<u8>) {
    let mut bios = vec![0; 8 * 1024];
    bios[..31].copy_from_slice(&[
        0x3E, 0x80, 0xD3, 0xE0, 0x3E, 0x04, 0xD3, 0xE0, 0x3E, 0x90, 0xD3, 0xE0, 0x3E, 0xBF, 0xD3,
        0xE0, 0x3E, 0xDF, 0xD3, 0xE0, 0x3E, 0xFF, 0xD3, 0xE0, 0x21, 0x00, 0x60, 0x34, 0xC3, 0x1B,
        0x00,
    ]);
    let mut rom = vec![0; 8 * 1024];
    rom[..2].copy_from_slice(&[0xAA, 0x55]);
    // A compact Z80 loop that mutates cartridge-visible execution state.
    rom[2..10].copy_from_slice(&[0x21, 0x00, 0x60, 0x34, 0x7E, 0xD3, 0xE0, 0x18]);
    rom[10] = 0xF7;
    (rom, bios)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn manifest_for(fixtures: &[Fixture]) -> CorpusManifest {
    CorpusManifest {
        schema: 1,
        fixtures: fixtures
            .iter()
            .map(|fixture| ManifestFixture {
                id: fixture.id.to_owned(),
                system: fixture.system.to_owned(),
                path: format!("{}.{}", fixture.id, fixture.extension),
                sha256: sha256_hex(&fixture.bytes),
                frames: SUGGESTED_FRAMES,
            })
            .collect(),
    }
}

/// Writes a new directory only. Existing outputs are never overwritten.
pub fn write_fresh_corpus(output_dir: &Path) -> io::Result<CorpusManifest> {
    fs::create_dir(output_dir)?;
    let fixtures = fixtures();
    let manifest = manifest_for(&fixtures);
    for (fixture, entry) in fixtures.iter().zip(&manifest.fixtures) {
        fs::write(output_dir.join(&entry.path), &fixture.bytes)?;
    }
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| io::Error::other(format!("serialize manifest: {error}")))?;
    fs::write(output_dir.join("manifest.json"), manifest_bytes)?;
    Ok(manifest)
}

fn gb_rom(color: bool) -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    // Disable LCD, configure and trigger square channel 1, then re-enable LCD
    // and keep mutating work RAM. This is guest code, not host initialization.
    let program = [
        0xF3, 0x3E, 0x00, 0xE0, 0x40, 0x3E, 0x80, 0xE0, 0x26, 0x3E, 0x77, 0xE0, 0x24, 0x3E, 0x11,
        0xE0, 0x25, 0x3E, 0x08, 0xE0, 0x10, 0x3E, 0x80, 0xE0, 0x11, 0x3E, 0xF3, 0xE0, 0x12, 0x3E,
        0x70, 0xE0, 0x13, 0x3E, 0x87, 0xE0, 0x14, 0x3E, 0x91, 0xE0, 0x40, 0xFA, 0x00, 0xC0, 0x3C,
        0xEA, 0x00, 0xC0, 0x18, 0xF7,
    ];
    rom[0x100..0x100 + program.len()].copy_from_slice(&program);
    rom[0x134..0x13D].copy_from_slice(if color {
        b"PGO-GBC\0\0"
    } else {
        b"PGO-GB\0\0\0"
    });
    rom[0x143] = if color { 0x80 } else { 0 };
    let mut checksum = 0_u8;
    for &byte in &rom[0x134..=0x14C] {
        checksum = checksum.wrapping_sub(byte).wrapping_sub(1);
    }
    rom[0x14D] = checksum;
    rom
}

fn nes_rom() -> Vec<u8> {
    let mut rom = vec![0; 16 + 0x4000 + 0x2000];
    rom[..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let prg = 16;
    let program = [
        0x78, 0xD8, 0xA2, 0x40, 0x8E, 0x17, 0x40, 0xA2, 0xFF, 0x9A, 0xA9, 0x00, 0x8D, 0x00, 0x20,
        0xA9, 0x1E, 0x8D, 0x01, 0x20, 0xA9, 0x3F, 0x8D, 0x00, 0x40, 0xA9, 0xFF, 0x8D, 0x02, 0x40,
        0xA9, 0x01, 0x8D, 0x15, 0x40, 0xA9, 0xF8, 0x8D, 0x03, 0x40, 0xEE, 0x00, 0x00, 0x4C, 0x28,
        0x80,
    ];
    rom[prg..prg + program.len()].copy_from_slice(&program);
    rom[prg + 0x3FFC..prg + 0x3FFE].copy_from_slice(&0x8000_u16.to_le_bytes());
    rom
}

fn pce_rom(memory: bool) -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    let program: &[u8] = if memory {
        &[0xD4, 0xA9, 0x5A, 0x8D, 0x00, 0x20, 0x1A, 0x80, 0xFA]
    } else {
        &[
            0xD4, 0xA9, 0xFF, 0x53, 0x01, 0xA9, 0x00, 0x8D, 0x00, 0x08, 0xA9, 0xFF, 0x8D, 0x01,
            0x08, 0xA9, 0x40, 0x8D, 0x02, 0x08, 0xA9, 0x00, 0x8D, 0x03, 0x08, 0xA9, 0x9F, 0x8D,
            0x04, 0x08, 0xA9, 0xFF, 0x8D, 0x05, 0x08, 0xA9, 0x1F, 0x8D, 0x06, 0x08, 0x80, 0xDB,
        ]
    };
    rom[..program.len()].copy_from_slice(program);
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    rom
}

fn sega8_rom(game_gear: bool) -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    let program = [
        0xF3, 0x31, 0xF0, 0xDF, 0x3E, 0x80, 0xD3, 0x7F, 0x3E, 0x40, 0xD3, 0xBF, 0x3E, 0x81, 0xD3,
        0xBF, 0x3E, 0x90, 0xD3, 0x7F, 0x3A, 0x00, 0xC0, 0x3C, 0x32, 0x00,
    ];
    rom[..program.len()].copy_from_slice(&program);
    rom[program.len()] = 0xC0;
    rom[program.len() + 1..program.len() + 4].copy_from_slice(&[0x18, 0xF8, 0x00]);
    if game_gear {
        rom[0x7FF0..0x7FF8].copy_from_slice(b"TMR SEGA");
    }
    rom
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn normal_corpus_has_every_non_firmware_system_and_unique_paths() {
        let fixtures = fixtures();
        let systems: BTreeSet<_> = fixtures.iter().map(|fixture| fixture.system).collect();
        assert_eq!(
            systems,
            BTreeSet::from(["GB", "GBC", "GBA", "GG", "NES", "PCE", "SMS", "WS", "WSC"])
        );
        let paths: BTreeSet<_> = manifest_for(&fixtures)
            .fixtures
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        assert_eq!(paths.len(), fixtures.len());
        assert!(
            paths
                .iter()
                .all(|path| Path::new(path).is_relative() && !path.contains(".."))
        );
    }

    #[test]
    fn gba_variants_have_headers_branches_and_distinct_training_programs() {
        let gba: Vec<_> = fixtures()
            .into_iter()
            .filter(|fixture| fixture.system == "GBA")
            .collect();
        assert_eq!(gba.len(), 3);
        assert!(
            gba.iter()
                .all(|fixture| fixture.bytes[0xA0..0xA4] == *b"ZPGO")
        );
        assert!(gba.iter().all(|fixture| fixture.bytes[..4] != [0, 0, 0, 0]));
        assert_ne!(gba[0].bytes, gba[1].bytes);
        assert_ne!(gba[1].bytes, gba[2].bytes);
    }

    #[test]
    fn manifest_is_schema_one_and_hashes_the_exact_bytes() {
        let fixtures = fixtures();
        let manifest = manifest_for(&fixtures);
        assert_eq!(manifest.schema, 1);
        for (fixture, entry) in fixtures.iter().zip(&manifest.fixtures) {
            assert_eq!(entry.sha256, sha256_hex(&fixture.bytes));
            assert_eq!(entry.frames, SUGGESTED_FRAMES);
            assert_eq!(entry.path, format!("{}.{}", fixture.id, fixture.extension));
        }
    }

    #[test]
    fn gba_active_variants_emit_pixels_and_nonzero_audio() {
        for fixture in fixtures()
            .into_iter()
            .filter(|fixture| matches!(fixture.id, "gba-arm-active" | "gba-thumb-active"))
        {
            let mut emulator = zeff_gba_core::emulator::Emulator::from_rom_data(&fixture.bytes)
                .unwrap_or_else(|error| panic!("{} did not load: {error}", fixture.id));
            emulator.set_apu_sample_generation_enabled(true);
            for _ in 0..120 {
                emulator.step_frame();
            }
            assert!(
                emulator
                    .framebuffer()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[..3] != [0, 0, 0]),
                "{} produced no visible pixels",
                fixture.id
            );
            let mut audio = Vec::new();
            emulator.drain_audio_samples_into(&mut audio);
            assert!(
                audio.iter().any(|sample| *sample != 0.0),
                "{} produced silent audio",
                fixture.id
            );
        }
    }

    #[test]
    fn other_core_fixtures_load_and_run_their_guest_programs() {
        use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
        use zeff_sega8_core::hardware::cartridge::SystemHint;

        for fixture in fixtures()
            .into_iter()
            .filter(|fixture| matches!(fixture.system, "GB" | "GBC"))
        {
            let mut emu = zeff_gb_core::emulator::Emulator::from_rom_data(
                &fixture.bytes,
                HardwareModePreference::Auto,
            )
            .unwrap();
            emu.set_apu_sample_generation_enabled(true);
            for _ in 0..120 {
                emu.step_frame();
            }
            let mut audio = Vec::new();
            emu.drain_audio_samples_into(&mut audio);
            assert!(
                audio.iter().any(|sample| *sample != 0.0),
                "{} silent",
                fixture.id
            );
        }
        let nes = fixtures()
            .into_iter()
            .find(|fixture| fixture.system == "NES")
            .unwrap();
        let mut nes_emu = zeff_nes_core::emulator::Emulator::from_rom_data(&nes.bytes).unwrap();
        nes_emu.set_apu_sample_generation_enabled(true);
        for _ in 0..120 {
            nes_emu.step_frame();
        }
        let mut audio = Vec::new();
        nes_emu.drain_audio_samples_into(&mut audio);
        assert!(audio.iter().any(|sample| *sample != 0.0), "NES silent");

        for fixture in fixtures()
            .into_iter()
            .filter(|fixture| fixture.system == "PCE")
        {
            let mut emu = zeff_pce_core::hardware::PceMachine::new(fixture.bytes).unwrap();
            emu.set_sample_rate(48_000);
            for _ in 0..120 {
                emu.run_until_frame().unwrap();
            }
            let mut audio = Vec::new();
            emu.drain_audio_samples_into(&mut audio);
            if fixture.id == "pce-video-loop" {
                assert!(audio.iter().any(|sample| *sample != 0.0), "PCE PSG silent");
            }
        }
        for fixture in fixtures()
            .into_iter()
            .filter(|fixture| matches!(fixture.system, "SMS" | "GG"))
        {
            let hint = if fixture.system == "GG" {
                SystemHint::GameGear
            } else {
                SystemHint::MasterSystem
            };
            let mut emu =
                zeff_sega8_core::emulator::Emulator::new_with_hint(&fixture.bytes, 48_000, hint)
                    .unwrap();
            emu.set_apu_sample_generation_enabled(true);
            for _ in 0..120 {
                emu.step_frame();
            }
            let mut audio = Vec::new();
            emu.drain_audio_samples_into(&mut audio);
            assert!(
                audio.iter().any(|sample| *sample != 0.0),
                "{} silent",
                fixture.id
            );
        }
    }

    #[test]
    fn wonderswan_guests_drive_cpu_ram_video_and_late_audio() {
        for fixture in fixtures()
            .into_iter()
            .filter(|fixture| matches!(fixture.system, "WS" | "WSC"))
        {
            let mut emu = zeff_ws_core::emulator::Emulator::from_rom_data(&fixture.bytes)
                .unwrap_or_else(|error| panic!("{} did not load: {error}", fixture.id));
            emu.set_apu_sample_generation_enabled(true);
            emu.set_opcode_log_enabled(true);
            let mut observed_opcodes = BTreeSet::new();
            for _ in 0..90 {
                emu.step_frame();
                observed_opcodes.extend(
                    emu.recent_opcodes(128)
                        .into_iter()
                        .map(|record| record.opcode),
                );
            }
            let early_cycles = emu.cpu_cycles();
            let ram = emu.system_ram();
            let ram_values = ram.iter().copied().collect::<BTreeSet<_>>().len();
            assert!(
                ram_values > 8,
                "{} did not execute its arithmetic RAM fill (only {ram_values} values)",
                fixture.id,
            );
            let sprite_range = if fixture.system == "WSC" {
                0x6000..0x7000
            } else {
                0x2000..0x3000
            };
            assert!(
                ram[0x1000..0x2000].iter().any(|&byte| byte != 0)
                    && ram[sprite_range].iter().any(|&byte| byte != 0),
                "{} did not reach its bounded tile and sprite memory ranges",
                fixture.id
            );
            let ram_len = ram.len();
            assert!(
                observed_opcodes.len() >= 10
                    && observed_opcodes.contains(&0x8A)
                    && observed_opcodes.contains(&0x88)
                    && observed_opcodes.contains(&0x75)
                    && observed_opcodes.contains(&0xA4),
                "{} did not execute diverse RAM/string/read/ALU/write/branch guest work: {observed_opcodes:?}",
                fixture.id
            );
            let first_pixel = &emu.framebuffer()[..4];
            assert!(
                emu.framebuffer()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel != first_pixel),
                "{} produced a flat framebuffer",
                fixture.id
            );
            let mut early_audio = Vec::new();
            emu.drain_audio_samples_into(&mut early_audio);
            // Trace only after initialization. A full high-byte sweep covers
            // each steady-state work window, making an accidental BX carry
            // into an unmapped mono address an observable test failure.
            let mut late_guest_addresses = Vec::new();
            for _ in 0..16_000 {
                let (_, accesses) = emu.step_instruction_with_bus_trace();
                late_guest_addresses.extend(
                    accesses
                        .into_iter()
                        .map(|access| access.addr())
                        .filter(|&address| address <= 0xFFFF),
                );
            }
            assert!(
                !late_guest_addresses.is_empty()
                    && late_guest_addresses
                        .iter()
                        .all(|&address| usize::try_from(address)
                            .is_ok_and(|address| address < ram_len)),
                "{} steady loop accessed outside its {ram_len}-byte internal RAM (max traced address {:#x})",
                fixture.id,
                late_guest_addresses
                    .iter()
                    .copied()
                    .max()
                    .unwrap_or_default(),
            );
            for _ in 0..30 {
                emu.step_frame();
            }
            assert!(
                emu.cpu_cycles() > early_cycles,
                "{} stopped executing",
                fixture.id
            );
            let mut late_audio = Vec::new();
            emu.drain_audio_samples_into(&mut late_audio);
            assert!(
                late_audio.iter().any(|sample| *sample != 0.0),
                "{} produced no sustained PCM after initialization",
                fixture.id
            );
        }
    }

    #[test]
    fn coleco_fixture_is_explicit_synthetic_data() {
        let (rom, bios) = coleco_fixture();
        assert_eq!(bios.len(), 8 * 1024);
        assert_eq!(rom.len(), 8 * 1024);
        assert_eq!(&rom[..2], &[0xAA, 0x55]);
        assert_eq!(&bios[..3], &[0x3E, 0x80, 0xD3]);
    }
}
