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
            heading: "Header",
            fields: vec![
                ("Title", header.title.clone()),
                ("Manufacturer", manufacturer),
                ("Publisher", header.publisher().to_string()),
                ("Cartridge", format!("{:?}", header.cartridge_type)),
                ("ROM Size", format!("{:?}", header.rom_size)),
                ("RAM Size", format!("{:?}", header.ram_size)),
            ],
        },
        RomInfoSection {
            heading: "Compatibility",
            fields: vec![
                ("Hardware Mode", format!("{:?}", emu.hardware_mode())),
                ("CGB Flag", format!("{:02X}", header.cgb_flag)),
                ("SGB Flag", format!("{:02X}", header.sgb_flag)),
                ("CGB Compatible", yes_no(header.is_cgb_compatible).into()),
                ("CGB Exclusive", yes_no(header.is_cgb_exclusive).into()),
                ("SGB Supported", yes_no(header.is_sgb_supported).into()),
            ],
        },
        RomInfoSection {
            heading: "Checksums",
            fields: vec![
                (
                    "Header",
                    pass_fail(header.verify_header_checksum(rom_bytes)).into(),
                ),
                (
                    "Global",
                    pass_fail(header.verify_global_checksum(rom_bytes)).into(),
                ),
                ("CRC32", format!("{:08X}", rom_crc32)),
            ],
        },
    ];

    sections.push(super::super::build_libretro_section(rom_crc32, platform));

    let mut cart_fields: Vec<(&'static str, String)> = vec![
        ("Mapper", cart_state.mapper.to_string()),
        ("ROM Bank", format!("{}", cart_state.active_rom_bank)),
        ("RAM Bank", format!("{}", cart_state.active_ram_bank)),
        ("RAM Enabled", yes_no(cart_state.ram_enabled).into()),
    ];
    if let Some(mode) = cart_state.banking_mode {
        cart_fields.push(("Banking Mode", if mode { "RAM" } else { "ROM" }.into()));
    }
    sections.push(RomInfoSection {
        heading: "Cartridge State",
        fields: cart_fields,
    });

    RomDebugInfo { sections }
}
