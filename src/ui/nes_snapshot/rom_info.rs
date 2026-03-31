use crate::debug::{RomDebugInfo, RomInfoSection};

pub(super) fn nes_rom_info(emu: &zeff_nes_core::emulator::Emulator) -> RomDebugInfo {
    let header = emu.cartridge_header();
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

    RomDebugInfo { sections }
}
