use crate::debug::{PerfInfo, RomDebugInfo, RomInfoSection};
use std::borrow::Cow;
use zeff_ws_core::emulator::Emulator;
use zeff_ws_core::hardware::cartridge::{MinimumSystem, RomFooter, RomOrientation};

pub(super) fn ws_perf_snapshot(emu: &Emulator) -> PerfInfo {
    PerfInfo {
        fps: 0.0,
        target_fps: zeff_emu_common::system::System::WonderSwan.target_fps(),
        speed_mode_label: super::super::normal_speed_mode_label(),
        frames_in_flight: 0,
        cycles: emu.cpu_cycles(),
        platform_name: "WonderSwan",
        hardware_label: hardware_label(emu.footer()),
        hardware_pref_label: "Auto".into(),
    }
}

pub(super) fn ws_rom_info(emu: &Emulator) -> RomDebugInfo {
    let footer = emu.footer();
    RomDebugInfo {
        sections: vec![
            RomInfoSection {
                heading: "WonderSwan Footer",
                fields: vec![
                    ("CRC32", format!("{:08X}", emu.rom_crc32())),
                    ("Developer ID", format!("{:02X}", footer.developer_id)),
                    ("Minimum System", format!("{:?}", footer.minimum_system)),
                    ("Cartridge ID", format!("{:02X}", footer.cartridge_id)),
                    ("Revision", footer.revision.to_string()),
                    ("ROM Size", rom_size_label(footer)),
                    ("Save", format!("{:?}", footer.save_kind)),
                    ("Flags", format!("{:02X}", footer.flags)),
                    ("Orientation", format!("{:?}", footer.orientation())),
                    ("RTC", super::on_off(footer.rtc_present).into()),
                    (
                        "Checksum",
                        format!(
                            "{:04X} computed={:04X} {}",
                            footer.checksum,
                            footer.computed_checksum,
                            if footer.checksum_valid {
                                "valid"
                            } else {
                                "invalid"
                            }
                        ),
                    ),
                ],
            },
            RomInfoSection {
                heading: "Core",
                fields: vec![(
                    "Status",
                    "Experimental WonderSwan/WonderSwan Color interpreter".into(),
                )],
            },
        ],
    }
}

fn hardware_label(footer: &RomFooter) -> Cow<'static, str> {
    let system = match footer.minimum_system {
        MinimumSystem::WonderSwan => "WonderSwan",
        MinimumSystem::WonderSwanColor => "WonderSwan Color",
        MinimumSystem::Unknown(_) => "WonderSwan (unknown minimum system)",
    };
    let orientation = match footer.orientation() {
        RomOrientation::Horizontal => "horizontal",
        RomOrientation::Vertical => "vertical",
    };
    format!("{system}, {orientation}").into()
}

fn rom_size_label(footer: &RomFooter) -> String {
    match footer.rom_size.declared_bytes {
        Some(bytes) => format!("{} bytes (code {:02X})", bytes, footer.rom_size.code),
        None => format!("unknown (code {:02X})", footer.rom_size.code),
    }
}
