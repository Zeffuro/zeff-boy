use super::*;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_emu_common::memory::MemoryRegionKind;
use zeff_emu_common::system::{CoreFamily, System};

#[path = "tests/pce_host.rs"]
mod pce_host;

#[path = "tests/nes_timing.rs"]
mod nes_timing;

#[path = "tests/nes_capacity.rs"]
mod nes_capacity;

fn load_sega8(ext: &str) -> CoreState {
    CoreState::from_rom(&[0x76], &format!("test.{ext}")).expect("Sega 8-bit ROM should load")
}

fn ws_rom() -> Vec<u8> {
    let mut rom = vec![0xFF; 0x10000];
    rom[0..2].copy_from_slice(&[0x90, 0xF4]);
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer + 4] = 0x01;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn gba_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom
}

fn emerald_flash1m_rom() -> Vec<u8> {
    let mut rom = gba_rom();
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    rom.extend_from_slice(b"FLASH1M_V103");
    rom
}

fn gba_backup_rom(marker: &[u8]) -> Vec<u8> {
    let mut rom = gba_rom();
    rom.extend_from_slice(marker);
    rom
}

fn nes_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;

    let prg = 16;
    rom[prg] = 0xA9;
    rom[prg + 1] = 0x42;
    rom[prg + 2] = 0x85;
    rom[prg + 3] = 0x00;
    rom[prg + 4] = 0xEA;
    rom[prg + 5] = 0xEA;
    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom
}

fn gb_rom() -> Vec<u8> {
    vec![0u8; 0x8000]
}

fn pce_rom() -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[0x1FFE..0x2000].copy_from_slice(&0xE000_u16.to_le_bytes());
    rom
}

fn codemasters_rom(bank_count: usize) -> Vec<u8> {
    use zeff_sega8_core::hardware::constants::{
        CODEMASTERS_HEADER_OFFSET, CODEMASTERS_HEADER_SIZE, ROM_BANK_SIZE,
    };

    let mut rom = vec![0; bank_count * ROM_BANK_SIZE];
    for bank in 0..bank_count {
        rom[bank * ROM_BANK_SIZE..(bank + 1) * ROM_BANK_SIZE].fill(bank as u8);
    }

    let offset = CODEMASTERS_HEADER_OFFSET;
    rom[offset] = bank_count as u8;
    rom[offset + 1] = 0x31;
    rom[offset + 2] = 0x08;
    rom[offset + 3] = 0x93;
    rom[offset + 4] = 0x10;
    rom[offset + 5] = 0x59;
    rom[offset + 6..offset + 8].copy_from_slice(&0x1234u16.to_le_bytes());
    rom[offset + 8..offset + 10].copy_from_slice(&0xEDCCu16.to_le_bytes());
    rom[offset + 10..offset + CODEMASTERS_HEADER_SIZE].fill(0);
    rom
}

#[path = "tests/cheats.rs"]
mod cheats;
#[path = "tests/gba_backup.rs"]
mod gba_backup;
#[path = "tests/md.rs"]
mod md;
#[path = "tests/md_abi.rs"]
mod md_abi;
#[path = "tests/memory.rs"]
mod memory;
#[path = "tests/pce_runtime.rs"]
mod pce_runtime;
#[path = "tests/sega8_loading.rs"]
mod sega8_loading;
#[path = "tests/system_matrix.rs"]
mod system_matrix;

fn rom_for_system(system: System) -> Vec<u8> {
    match system {
        System::Md => md::rom(b'U', &[0x60fe, 0x4e71], false),
        System::Gb => gb_rom(),
        System::Gba => gba_rom(),
        System::Nes => nes_rom(),
        System::Coleco => vec![0xAA, 0x55],
        System::Pce => pce_rom(),
        System::Ws => ws_rom(),
        System::Sms | System::Gg | System::Sg => vec![0x76],
    }
}

fn active_core_family(state: &CoreState) -> CoreFamily {
    match &state.core {
        ActiveCore::Md(_) => CoreFamily::MegaDrive,
        ActiveCore::Gb(_) => CoreFamily::GameBoy,
        ActiveCore::Gba(_) => CoreFamily::GameBoyAdvance,
        ActiveCore::Nes(_) => CoreFamily::Nes,
        ActiveCore::Pce(_) => CoreFamily::PcEngine,
        ActiveCore::Sega8(_) => CoreFamily::Sega8,
        ActiveCore::Ws(_) => CoreFamily::WonderSwan,
    }
}

fn expected_system_label(system: System) -> &'static str {
    match system {
        System::Md => "Mega Drive / Genesis",
        System::Gb => "GB/GBC",
        System::Gba => "GBA",
        System::Nes => "NES",
        System::Coleco => "ColecoVision",
        System::Pce => "PC Engine",
        System::Ws => "WonderSwan",
        System::Sms => "SMS",
        System::Gg => "Game Gear",
        System::Sg => "SG-1000/SC-3000",
    }
}

fn expected_system_ram_size(system: System) -> usize {
    match system {
        System::Md => zeff_md_core::constants::WORK_RAM_SIZE,
        System::Gb => zeff_gb_core::hardware::types::constants::WRAM_SIZE * 8,
        System::Gba => zeff_gba_core::hardware::constants::SYSTEM_RAM_SIZE,
        System::Nes => zeff_nes_core::hardware::constants::SYSTEM_RAM_SIZE,
        System::Coleco => 1024,
        System::Pce => zeff_pce_core::hardware::WORK_RAM_LEN,
        System::Ws => zeff_ws_core::hardware::constants::WSC_INTERNAL_RAM_SIZE,
        System::Sms | System::Gg => zeff_sega8_core::hardware::constants::SMS_WORK_RAM_SIZE,
        System::Sg => zeff_sega8_core::hardware::constants::SG_WORK_RAM_SIZE,
    }
}

fn assert_copyable_regions(state: &mut CoreState) {
    let regions = state.memory_regions();
    let mut copied = Vec::new();

    for region in regions {
        if region.kind == MemoryRegionKind::CpuAddressSpace {
            assert!(
                state.copy_memory_region(region.id, &mut copied).is_err(),
                "CPU address spaces should not be copied as finite memory regions"
            );
            continue;
        }

        let copied_region = state
            .copy_memory_region(region.id, &mut copied)
            .unwrap_or_else(|err| {
                panic!(
                    "copying libretro memory region '{}' failed: {err}",
                    region.id
                )
            });
        assert_eq!(copied_region, region);
        assert_eq!(copied.len(), region.size.unwrap_or_default());

        if let Some(alias) = region.aliases.first() {
            let alias_region = state
                .copy_memory_region(alias, &mut copied)
                .unwrap_or_else(|err| {
                    panic!(
                        "copying libretro memory region '{}' through alias '{}' failed: {err}",
                        region.id, alias
                    )
                });
            assert_eq!(alias_region, region);
            assert_eq!(copied.len(), region.size.unwrap_or_default());
        }
    }
}
