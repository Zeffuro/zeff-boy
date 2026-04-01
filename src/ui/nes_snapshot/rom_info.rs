use crate::debug::{RomDebugInfo, RomInfoSection};

pub(super) fn nes_rom_info(emu: &zeff_nes_core::emulator::Emulator) -> RomDebugInfo {
    let header = emu.cartridge_header();
    let rom_crc32 = emu.rom_crc32();
    let libretro_meta = crate::libretro_metadata::lookup_cached(
        rom_crc32,
        crate::libretro_common::LibretroPlatform::Nes,
    );
    let yes_no = |v: bool| if v { "Yes" } else { "No" };

    let chr_label = if header.chr_rom_size > 0 {
        format!("{} KiB", header.chr_rom_size / 1024)
    } else {
        "0 (CHR-RAM)".into()
    };

    let mut sections = vec![
        RomInfoSection {
            heading: "ROM Header".into(),
            fields: vec![
                ("Format".into(), format!("{:?}", header.format)),
                (
                    "PRG ROM".into(),
                    format!("{} KiB", header.prg_rom_size / 1024),
                ),
                ("CHR ROM".into(), chr_label),
                ("Mapper".into(), header.mapper_label()),
                ("Mirroring".into(), format!("{:?}", header.mirroring)),
                ("Battery".into(), yes_no(header.has_battery).into()),
                ("Trainer".into(), yes_no(header.has_trainer).into()),
            ],
        },
        RomInfoSection {
            heading: "System".into(),
            fields: vec![
                ("Console".into(), format!("{:?}", header.console_type)),
                ("Timing".into(), format!("{:?}", header.timing)),
            ],
        },
    ];

    if header.format == zeff_nes_core::hardware::cartridge::RomFormat::Nes2 {
        sections.push(RomInfoSection {
            heading: "NES 2.0 Extended".into(),
            fields: vec![
                ("PRG-RAM".into(), format!("{} B", header.prg_ram_size)),
                ("PRG-NVRAM".into(), format!("{} B", header.prg_nvram_size)),
                ("CHR-RAM".into(), format!("{} B", header.chr_ram_size)),
                ("CHR-NVRAM".into(), format!("{} B", header.chr_nvram_size)),
                ("Misc ROMs".into(), format!("{}", header.misc_roms)),
                (
                    "Expansion Device".into(),
                    format!("{}", header.default_expansion_device),
                ),
            ],
        });
    } else {
        sections.push(RomInfoSection {
            heading: "RAM".into(),
            fields: vec![("PRG-RAM".into(), format!("{} B", header.prg_ram_size))],
        });
    }

    sections.push(RomInfoSection {
        heading: "Checksums".into(),
        fields: vec![("CRC32".into(), format!("{rom_crc32:08X}"))],
    });

    let libretro_fields = match &libretro_meta {
        Some(meta) => vec![
            ("Title".into(), meta.title.clone()),
            ("ROM File".into(), meta.rom_name.clone()),
        ],
        None => vec![("Status".into(), "No local metadata match".into())],
    };
    sections.push(RomInfoSection {
        heading: "libretro Metadata".into(),
        fields: libretro_fields,
    });

    RomDebugInfo { sections }
}
