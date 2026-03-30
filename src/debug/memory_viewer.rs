use super::common::{parse_hex_u8, parse_hex_u16, HEX_PAGE_SIZE};
use super::hex_viewer;
use crate::debug::types::MemoryViewerState;

const MAX_START: u16 = 0xFF00;
const FLASH_DURATION_TICKS: u8 = 12;

pub(super) fn draw_memory_viewer_content(
    ui: &mut egui::Ui,
    state: &mut MemoryViewerState,
    memory_page: &[(u16, u8)],
) -> Vec<(u16, u8)> {
    let mut writes = Vec::new();

    sync_flash_state(state, memory_page);

    ui.horizontal(|ui| {
        ui.label("Address:");
        let response = ui.text_edit_singleline(&mut state.jump_input);
        let input_has_focus = response.has_focus();
        let pressed_enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Go").clicked() || pressed_enter)
            && let Some(addr) = parse_hex_u16(&state.jump_input) {
                state.view_start = addr & 0xFFF0;
                state.jump_input = format!("{:04X}", state.view_start);
            }

        if !input_has_focus {
            state.jump_input = format!("{:04X}", state.view_start);
        }
    });

    ui.horizontal(|ui| {
        if ui.button("-0x10").clicked() {
            state.view_start = state.view_start.saturating_sub(0x10);
        }
        if ui.button("+0x10").clicked() {
            state.view_start = state.view_start.saturating_add(0x10).min(MAX_START);
        }
        if ui.button("-0x100").clicked() {
            state.view_start = state.view_start.saturating_sub(0x100);
        }
        if ui.button("+0x100").clicked() {
            state.view_start = state.view_start.saturating_add(0x100).min(MAX_START);
        }
    });

    state.view_start =
        hex_viewer::handle_scroll(ui, state.view_start as u32, MAX_START as u32) as u16;

    let slider = ui.add(
        egui::Slider::new(&mut state.view_start, 0..=MAX_START)
            .step_by(16.0)
            .text("Start"),
    );
    state.view_start &= 0xFFF0;
    if slider.changed() {
        state.jump_input = format!("{:04X}", state.view_start);
    }

    ui.separator();

    let fmt = hex_viewer::hex_text_formats(ui);
    hex_viewer::draw_hex_header(ui, "Addr   ", &fmt);
    hex_viewer::draw_hex_grid(
        ui,
        memory_page,
        4,
        &fmt,
        Some(&state.flash_ticks),
        &state.tbl_map,
    );

    if state.enable_editing {
        ui.separator();
        if let Some(addr) = state.edit_addr {
            ui.horizontal(|ui| {
                ui.monospace(format!("Edit {:04X}:", addr));
                ui.add(
                    egui::TextEdit::singleline(&mut state.edit_value)
                        .desired_width(50.0)
                        .char_limit(2),
                );
                if ui.button("Write").clicked() {
                    if let Some(value) = parse_hex_u8(&state.edit_value) {
                        writes.push((addr, value));
                    }
                    state.edit_addr = None;
                }
                if ui.button("Cancel").clicked() {
                    state.edit_addr = None;
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label("Edit addr:");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut state.edit_addr_input)
                    .desired_width(60.0)
                    .char_limit(4)
                    .hint_text("hex addr"),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Some(addr) = parse_hex_u16(&state.edit_addr_input) {
                    state.edit_addr = Some(addr);
                    let val = memory_page
                        .iter()
                        .find(|(a, _)| *a == addr)
                        .map(|(_, v)| *v)
                        .unwrap_or(0);
                    state.edit_value = format!("{:02X}", val);
                }
        });
    }

    ui.separator();
    if let Some(jump) = hex_viewer::draw_search_section(
        ui,
        "🔍 Search Memory",
        "search_mode",
        &mut state.search_mode,
        &mut state.search_query,
        &mut state.search_max_results,
        &mut state.search_pending,
        &state.search_results,
    ) {
        state.view_start = (jump as u16) & 0xFFF0;
        state.jump_input = format!("{:04X}", state.view_start);
    }

    ui.separator();
    hex_viewer::draw_tbl_section(ui, &mut state.tbl_map, &mut state.tbl_path);

    writes
}

fn sync_flash_state(state: &mut MemoryViewerState, memory_page: &[(u16, u8)]) {
    if state.flash_ticks.len() != HEX_PAGE_SIZE {
        state.flash_ticks = vec![0; HEX_PAGE_SIZE];
    }

    let page_addr = memory_page.first().map(|(a, _)| *a);
    let same_page = page_addr == Some(state.view_start)
        && state.prev_start == page_addr
        && state.prev_bytes.len() == memory_page.len();

    if same_page {
        for (i, (_, value)) in memory_page.iter().enumerate() {
            if *value != state.prev_bytes[i] {
                state.flash_ticks[i] = FLASH_DURATION_TICKS;
            } else if state.flash_ticks[i] > 0 {
                state.flash_ticks[i] -= 1;
            }
        }
    } else {
        for tick in &mut state.flash_ticks {
            *tick = 0;
        }
    }

    state.prev_start = page_addr;
    state.prev_bytes.clear();
    state.prev_bytes.extend(memory_page.iter().map(|(_, v)| *v));
}


