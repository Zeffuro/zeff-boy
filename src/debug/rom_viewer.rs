use super::common::parse_hex_u32;
use super::hex_viewer;
use crate::debug::types::RomViewerState;

const ROM_BANK_SIZE: u32 = 0x4000;

pub(super) fn draw_rom_viewer_content(
    ui: &mut egui::Ui,
    state: &mut RomViewerState,
    rom_page: &[(u32, u8)],
    rom_size: u32,
) {
    state.rom_size = rom_size;

    if rom_size == 0 {
        ui.label("No ROM loaded");
        return;
    }

    let max_start = rom_size.saturating_sub(super::common::HEX_PAGE_SIZE as u32) & !0xF;

    let banks = rom_size / ROM_BANK_SIZE;
    ui.label(format!(
        "ROM: {} bytes ({} banks × 16 KiB)",
        rom_size, banks
    ));
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Offset:");
        let response = ui.text_edit_singleline(&mut state.jump_input);
        let input_has_focus = response.has_focus();
        let pressed_enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Go").clicked() || pressed_enter)
            && let Some(addr) = parse_hex_u32(&state.jump_input) {
                state.view_start = (addr & !0xF).min(max_start);
                state.jump_input = format!("{:06X}", state.view_start);
            }

        if !input_has_focus {
            state.jump_input = format!("{:06X}", state.view_start);
        }
    });

    ui.horizontal(|ui| {
        if ui.button("-0x10").clicked() {
            state.view_start = state.view_start.saturating_sub(0x10);
        }
        if ui.button("+0x10").clicked() {
            state.view_start = state.view_start.saturating_add(0x10).min(max_start);
        }
        if ui.button("-0x100").clicked() {
            state.view_start = state.view_start.saturating_sub(0x100);
        }
        if ui.button("+0x100").clicked() {
            state.view_start = state.view_start.saturating_add(0x100).min(max_start);
        }
        if ui.button("-Bank").clicked() {
            state.view_start = state.view_start.saturating_sub(ROM_BANK_SIZE);
        }
        if ui.button("+Bank").clicked() {
            state.view_start = state.view_start.saturating_add(ROM_BANK_SIZE).min(max_start);
        }
    });

    state.view_start = hex_viewer::handle_scroll(ui, state.view_start, max_start);

    let bank = state.view_start / ROM_BANK_SIZE;
    ui.label(format!("Bank: {} (0x{:02X})", bank, bank));

    ui.separator();

    let fmt = hex_viewer::hex_text_formats(ui);
    hex_viewer::draw_hex_header(ui, "Offset   ", &fmt);
    hex_viewer::draw_hex_grid(ui, rom_page, 6, &fmt, None, &state.tbl_map);

    ui.separator();
    if let Some(jump) = hex_viewer::draw_search_section(
        ui,
        "🔍 Search ROM",
        "rom_search_mode",
        &mut state.search_mode,
        &mut state.search_query,
        &mut state.search_max_results,
        &mut state.search_pending,
        &state.search_results,
    ) {
        state.view_start = jump & !0xF;
        state.jump_input = format!("{:06X}", state.view_start);
    }

    ui.separator();
    hex_viewer::draw_tbl_section(ui, &mut state.tbl_map, &mut state.tbl_path);
}
