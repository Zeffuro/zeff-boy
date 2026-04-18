use crate::debug::types::RomDebugInfo;

pub(super) fn draw_rom_info_content(ui: &mut egui::Ui, info: &RomDebugInfo) {
    for section in &info.sections {
        ui.heading(section.heading);
        for (key, value) in &section.fields {
            ui.monospace(format!("{}: {}", key, value));
        }
        ui.separator();
    }
}
