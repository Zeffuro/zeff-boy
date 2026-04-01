use crate::debug::{RomDebugInfo, RomInfoSection};
use zeff_gb_core::emulator::Emulator;

pub(super) fn gb_rom_info(emu: &Emulator) -> RomDebugInfo {
    let header = emu.rom_info();
    let rom_bytes = emu.cartridge_rom_bytes();
    let rom_crc32 = crc32fast::hash(rom_bytes);
    let is_gbc = header.is_cgb_compatible || header.is_cgb_exclusive;
    let platform = if is_gbc {
        crate::libretro_common::LibretroPlatform::Gbc
    } else {
        crate::libretro_common::LibretroPlatform::Gb
    };
    let libretro_meta = crate::libretro_metadata::lookup_cached(rom_crc32, platform);
    let manufacturer = header
        .manufacturer_code
        .as_deref()
        .unwrap_or("N/A")
        .to_string();

    let yes_no = |v: bool| if v { "Yes" } else { "No" };
    let pass_fail = |v: bool| if v { "Valid" } else { "Invalid" };
    let cart_state = emu.cartridge_state();

    let mut sections = vec![
        RomInfoSection {
            heading: "Header".into(),
            fields: vec![
                ("Title".into(), header.title.clone()),
                ("Manufacturer".into(), manufacturer),
                ("Publisher".into(), header.publisher().to_string()),
                ("Cartridge".into(), format!("{:?}", header.cartridge_type)),
                ("ROM Size".into(), format!("{:?}", header.rom_size)),
                ("RAM Size".into(), format!("{:?}", header.ram_size)),
            ],
        },
        RomInfoSection {
            heading: "Compatibility".into(),
            fields: vec![
                ("Hardware Mode".into(), format!("{:?}", emu.hardware_mode())),
                ("CGB Flag".into(), format!("{:02X}", header.cgb_flag)),
                ("SGB Flag".into(), format!("{:02X}", header.sgb_flag)),
                (
                    "CGB Compatible".into(),
                    yes_no(header.is_cgb_compatible).into(),
                ),
                (
                    "CGB Exclusive".into(),
                    yes_no(header.is_cgb_exclusive).into(),
                ),
                (
                    "SGB Supported".into(),
                    yes_no(header.is_sgb_supported).into(),
                ),
            ],
        },
        RomInfoSection {
            heading: "Checksums".into(),
            fields: vec![
                (
                    "Header".into(),
                    pass_fail(header.verify_header_checksum(rom_bytes)).into(),
                ),
                (
                    "Global".into(),
                    pass_fail(header.verify_global_checksum(rom_bytes)).into(),
                ),
                ("CRC32".into(), format!("{:08X}", rom_crc32)),
            ],
        },
    ];

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

    let mut cart_fields: Vec<(String, String)> = vec![
        ("Mapper".into(), cart_state.mapper.to_string()),
        ("ROM Bank".into(), format!("{}", cart_state.active_rom_bank)),
        ("RAM Bank".into(), format!("{}", cart_state.active_ram_bank)),
        ("RAM Enabled".into(), yes_no(cart_state.ram_enabled).into()),
    ];
    if let Some(mode) = cart_state.banking_mode {
        cart_fields.push((
            "Banking Mode".into(),
            if mode { "RAM" } else { "ROM" }.into(),
        ));
    }
    sections.push(RomInfoSection {
        heading: "Cartridge State".into(),
        fields: cart_fields,
    });

    RomDebugInfo { sections }
}
