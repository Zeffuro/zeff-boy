use super::sega8_system_label;
use crate::debug::{PerfInfo, RomDebugInfo, RomInfoSection};
use zeff_sega8_core::emulator::Emulator;
use zeff_sega8_core::hardware::cartridge::{CodemastersHeader, RomHeader};

pub(super) fn sega8_perf_snapshot(emu: &Emulator) -> PerfInfo {
    PerfInfo {
        fps: 0.0,
        target_fps: f64::from(emu.video_standard().frame_rate_approx()),
        speed_mode_label: super::super::normal_speed_mode_label(),
        frames_in_flight: 0,
        cycles: emu.cpu().cycles(),
        platform_name: "Sega 8-bit",
        hardware_label: sega8_system_label(emu.system()).into(),
        hardware_pref_label: format!(
            "{}/{}",
            emu.video_standard().display_label(),
            emu.console_region().display_label()
        )
        .into(),
    }
}

pub(super) fn sega8_rom_info(emu: &Emulator) -> RomDebugInfo {
    let cart = &emu.bus().cartridge;
    let mut sections = vec![RomInfoSection {
        heading: "Sega 8-bit Cartridge",
        fields: vec![
            ("System", sega8_system_label(cart.system()).into()),
            (
                "Video Standard",
                format!(
                    "{} ({} scanlines/frame)",
                    emu.video_standard().display_label(),
                    emu.bus().vdp().total_scanlines()
                ),
            ),
            (
                "Console Region",
                emu.console_region().display_label().into(),
            ),
            ("Mapper", cart.mapper_kind().label().into()),
            ("Raw Size", format!("{} bytes", cart.raw_len())),
            (
                "Normalized Size",
                format!("{} bytes", cart.normalized_len()),
            ),
            (
                "Normalized CRC32",
                format!("{:08X}", cart.normalized_crc32()),
            ),
            ("ROM Banks", cart.rom_bank_count().to_string()),
            (
                "Copier Header",
                if cart.copier_header_stripped() {
                    "stripped"
                } else {
                    "absent"
                }
                .into(),
            ),
        ],
    }];

    sections.push(match cart.header() {
        Some(header) => sega8_header_section(header),
        None => RomInfoSection {
            heading: "Sega Header",
            fields: vec![("Status", "No TMR SEGA header found".into())],
        },
    });

    if let Some(header) = cart.codemasters_header() {
        sections.push(sega8_codemasters_header_section(header));
    }

    sections.push(RomInfoSection {
        heading: "Core",
        fields: vec![(
            "Status",
            "Experimental Sega Master System/Game Gear/SG-1000 core; Z80 interpreter and SMS/GG Mode 4 video are in active bring-up".into(),
        )],
    });

    RomDebugInfo { sections }
}

fn sega8_header_section(header: RomHeader) -> RomInfoSection {
    RomInfoSection {
        heading: "Sega Header",
        fields: vec![
            ("Location", format!("{:?}", header.location)),
            ("Checksum", format!("{:04X}", header.checksum)),
            (
                "Product Code BCD",
                format!(
                    "{:02X} {:02X} {:X}",
                    header.product_code_bcd[0],
                    header.product_code_bcd[1],
                    header.product_code_bcd[2]
                ),
            ),
            ("Version", header.version.to_string()),
            (
                "Region",
                format!("{:?} ({:X})", header.region, header.region.code()),
            ),
            ("ROM Size Code", format!("{:X}", header.rom_size_code)),
        ],
    }
}

fn sega8_codemasters_header_section(header: CodemastersHeader) -> RomInfoSection {
    RomInfoSection {
        heading: "Codemasters Header",
        fields: vec![
            ("Checksum Banks", header.checksum_bank_count.to_string()),
            (
                "Build Date BCD",
                format!(
                    "{:02X}/{:02X}/{:02X}",
                    header.day_bcd, header.month_bcd, header.year_bcd
                ),
            ),
            (
                "Build Time BCD",
                format!("{:02X}:{:02X}", header.hour_bcd, header.minute_bcd),
            ),
            ("Checksum", format!("{:04X}", header.checksum)),
            (
                "Checksum Complement",
                format!("{:04X}", header.checksum_complement),
            ),
        ],
    }
}
