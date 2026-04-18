use crate::debug::{RomDebugInfo, RomInfoSection};

pub(super) fn nes_rom_info(emu: &zeff_nes_core::emulator::Emulator) -> RomDebugInfo {
    let header = emu.cartridge_header();
    let rom_crc32 = emu.rom_crc32();
    let yes_no = |v: bool| if v { "Yes" } else { "No" };

    let chr_label = if header.chr_rom_size > 0 {
        format!("{} KiB", header.chr_rom_size / 1024)
    } else {
        "0 (CHR-RAM)".into()
    };

    let mut sections = vec![
        RomInfoSection {
            heading: "ROM Header",
            fields: vec![
                ("Format", format!("{:?}", header.format)),
                ("PRG ROM", format!("{} KiB", header.prg_rom_size / 1024)),
                ("CHR ROM", chr_label),
                ("Mapper", header.mapper_label()),
                ("Mirroring", format!("{:?}", header.mirroring)),
                ("Battery", yes_no(header.has_battery).into()),
                ("Trainer", yes_no(header.has_trainer).into()),
            ],
        },
        RomInfoSection {
            heading: "System",
            fields: vec![
                ("Console", format!("{:?}", header.console_type)),
                ("Timing", format!("{:?}", header.timing)),
            ],
        },
    ];

    if header.format == zeff_nes_core::hardware::cartridge::RomFormat::Nes2 {
        sections.push(RomInfoSection {
            heading: "NES 2.0 Extended",
            fields: vec![
                ("PRG-RAM", format!("{} B", header.prg_ram_size)),
                ("PRG-NVRAM", format!("{} B", header.prg_nvram_size)),
                ("CHR-RAM", format!("{} B", header.chr_ram_size)),
                ("CHR-NVRAM", format!("{} B", header.chr_nvram_size)),
                ("Misc ROMs", format!("{}", header.misc_roms)),
                (
                    "Expansion Device",
                    format!("{}", header.default_expansion_device),
                ),
            ],
        });
    } else {
        sections.push(RomInfoSection {
            heading: "RAM",
            fields: vec![("PRG-RAM", format!("{} B", header.prg_ram_size))],
        });
    }

    sections.push(RomInfoSection {
        heading: "Checksums",
        fields: vec![("CRC32", format!("{rom_crc32:08X}"))],
    });

    sections.push(super::super::build_libretro_section(
        rom_crc32,
        crate::libretro_common::LibretroPlatform::Nes,
    ));

    RomDebugInfo { sections }
}
