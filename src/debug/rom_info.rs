use crate::debug::RomInfoViewData;

pub(crate) fn draw_rom_info(ctx: &egui::Context, info: &RomInfoViewData, open: &mut bool) {
    egui::Window::new("ROM Info")
        .open(open)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.heading("Header");
            ui.monospace(format!("Title: {}", info.title));
            ui.monospace(format!("Manufacturer: {}", info.manufacturer));
            ui.monospace(format!("Publisher: {}", info.publisher));
            ui.monospace(format!("Cartridge: {}", info.cartridge_type));
            ui.monospace(format!("ROM Size: {}", info.rom_size));
            ui.monospace(format!("RAM Size: {}", info.ram_size));

            ui.separator();
            ui.heading("Compatibility");
            ui.monospace(format!(
                "Hardware Mode: {:?}  CGB Flag:{:02X}  SGB Flag:{:02X}",
                info.hardware_mode, info.cgb_flag, info.sgb_flag
            ));
            ui.monospace(format!(
                "CGB Compatible:{}  CGB Exclusive:{}  SGB Supported:{}",
                yes_no(info.is_cgb_compatible),
                yes_no(info.is_cgb_exclusive),
                yes_no(info.is_sgb_supported),
            ));

            ui.separator();
            ui.heading("Checksums");
            ui.monospace(format!(
                "Header: {}    Global: {}",
                pass_fail(info.header_checksum_valid),
                pass_fail(info.global_checksum_valid),
            ));

            ui.separator();
            ui.heading("Cartridge State");
            ui.monospace(format!("Mapper: {}", info.cartridge_state.mapper));
            ui.monospace(format!(
                "ROM Bank: {}  RAM Bank: {}",
                info.cartridge_state.active_rom_bank,
                info.cartridge_state.active_ram_bank
            ));
            ui.monospace(format!(
                "RAM Enabled: {}",
                yes_no(info.cartridge_state.ram_enabled)
            ));
            if let Some(mode) = info.cartridge_state.banking_mode {
                ui.monospace(format!(
                    "Banking Mode: {}",
                    if mode { "RAM" } else { "ROM" }
                ));
            }
        });
}

fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

fn pass_fail(value: bool) -> &'static str {
    if value { "Valid" } else { "Invalid" }
}
