use crate::debug::{PerfInfo, RomDebugInfo, RomInfoSection};
use sha2::{Digest, Sha256};
use zeff_coleco_core::Emulator;

pub(super) fn coleco_perf_snapshot(emu: &Emulator) -> PerfInfo {
    PerfInfo {
        fps: 0.0,
        target_fps: 60.0,
        speed_mode_label: super::super::normal_speed_mode_label(),
        frames_in_flight: 0,
        cycles: emu.effective_cycles(),
        platform_name: "ColecoVision",
        hardware_label: "NTSC ColecoVision".into(),
        hardware_pref_label: "Standard controller / cartridge".into(),
    }
}

pub(super) fn coleco_rom_info(emu: &Emulator) -> RomDebugInfo {
    let rom = emu.bus().cartridge();
    RomDebugInfo {
        sections: vec![
            RomInfoSection {
                heading: "ColecoVision Cartridge",
                fields: vec![
                    ("ROM Size", format!("{} bytes", rom.len())),
                    ("ROM SHA-256", hex_hash(Sha256::digest(rom).into())),
                    ("Address Range", "8000-FFFF".into()),
                ],
            },
            RomInfoSection {
                heading: "Core",
                fields: vec![
                    ("Region", "NTSC".into()),
                    ("Video", "TMS9928A".into()),
                    ("Audio", "SN76489A".into()),
                ],
            },
        ],
    }
}

fn hex_hash(hash: [u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}
