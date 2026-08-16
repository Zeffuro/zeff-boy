use super::common::{HEX_PAGE_SIZE, parse_hex_u8, parse_hex_u32};
use super::{data_inspector, hex_search, hex_viewer};
use crate::debug::types::MemoryViewerState;
use zeff_emu_common::address::Address;

const FLASH_DURATION_TICKS: u8 = 12;

pub(super) fn draw_memory_viewer_content(
    ui: &mut egui::Ui,
    state: &mut MemoryViewerState,
    memory_page: &[(Address, u8)],
    symbols: &crate::symbols::SymbolSession,
) -> Vec<(Address, u8)> {
    let mut writes = Vec::new();
    let address_space = state.address_space;
    state.view_start = address_space.clamp_start(state.view_start);

    sync_flash_state(state, memory_page);

    ui.horizontal_wrapped(|ui| {
        ui.label("Address:");
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.jump_input)
                .desired_width(110.0)
                .hint_text("hex or symbol"),
        );
        let input_has_focus = response.has_focus();
        let pressed_enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Go").clicked() || pressed_enter)
            && let Some(addr) = parse_hex_u32(&state.jump_input).or_else(|| {
                symbols
                    .resolve_cpu_name(state.jump_input.trim())
                    .and_then(|address| u32::try_from(address).ok())
            })
        {
            state.view_start = address_space.clamp_start(addr);
            state.jump_input = address_space.format(state.view_start);
        }

        if !input_has_focus {
            state.jump_input = address_space.format(state.view_start);
        }
    });

    ui.horizontal_wrapped(|ui| {
        if ui.button("-0x10").clicked() {
            state.view_start = state.view_start.saturating_sub(0x10);
        }
        if ui.button("+0x10").clicked() {
            state.view_start = address_space.clamp_start(state.view_start.saturating_add(0x10));
        }
        if ui.button("-0x100").clicked() {
            state.view_start = state.view_start.saturating_sub(0x100);
        }
        if ui.button("+0x100").clicked() {
            state.view_start = address_space.clamp_start(state.view_start.saturating_add(0x100));
        }
    });

    ui.label(format!(
        "Range: {}..{}",
        address_space.format(address_space.min),
        address_space.format(address_space.max_start.saturating_add(0xFF))
    ));
    if let Some(name) = symbols.symbol_name_at_cpu_address(state.view_start as u64) {
        ui.monospace(name);
    }

    let slider = ui.add(
        egui::Slider::new(
            &mut state.view_start,
            address_space.min..=address_space.max_start,
        )
        .step_by(16.0)
        .text("Start"),
    );
    state.view_start = address_space.clamp_start(state.view_start);
    if slider.changed() {
        state.jump_input = address_space.format(state.view_start);
    }

    ui.separator();

    let layout = hex_viewer::hex_layout(ui.available_width(), address_space.addr_width);
    let hex_block = ui.vertical(|ui| {
        let fmt = hex_viewer::hex_text_formats(ui, layout);
        hex_viewer::draw_hex_header(ui, "Addr   ", &fmt);
        hex_viewer::draw_hex_grid(
            ui,
            memory_page,
            address_space.addr_width,
            &fmt,
            Some(&state.flash_ticks),
            &state.tbl_map,
        );
    });
    let scrolled_start = hex_viewer::handle_scroll(
        ui,
        hex_block.response.rect,
        state.view_start,
        address_space.max_start,
        layout.bytes_per_row,
    );
    if scrolled_start != state.view_start {
        state.view_start = address_space.clamp_start(scrolled_start);
        state.jump_input = address_space.format(state.view_start);
    }

    if state.enable_editing {
        ui.separator();
        if let Some(addr) = state.edit_addr {
            ui.horizontal(|ui| {
                ui.monospace(format!("Edit {}:", address_space.format(addr)));
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
                    .desired_width(if address_space.addr_width <= 4 {
                        60.0
                    } else {
                        80.0
                    })
                    .char_limit(address_space.addr_width)
                    .hint_text("hex addr"),
            );
            if resp.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Some(addr) = parse_hex_u32(&state.edit_addr_input)
            {
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
    if let Some(jump) = hex_viewer::draw_bookmarks_section(
        ui,
        &mut state.bookmark_addr_input,
        &mut state.bookmark_label_input,
        &mut state.bookmarks,
        state.view_start,
    ) {
        state.view_start = address_space.clamp_start(jump);
        state.jump_input = address_space.format(state.view_start);
    }

    ui.separator();
    if let Some(jump) = hex_viewer::draw_diff_section(ui, &state.recent_diffs) {
        state.view_start = address_space.clamp_start(jump);
        state.jump_input = address_space.format(state.view_start);
    }

    ui.separator();
    if let Some(jump) = hex_search::draw_pattern_section(
        ui,
        &mut state.pattern_query,
        &mut state.pattern_max_results,
        &mut state.pattern_results,
        &mut state.pattern_error,
        memory_page,
    ) {
        state.view_start = address_space.clamp_start(jump);
        state.jump_input = address_space.format(state.view_start);
    }

    ui.separator();
    if let Some(jump) = hex_search::draw_search_section(
        ui,
        "🔍 Search Memory",
        "search_mode",
        &mut hex_search::SearchSectionParams {
            mode: &mut state.search_mode,
            query: &mut state.search_query,
            max_results: &mut state.search_max_results,
            pending: &mut state.search_pending,
        },
        &state.search_results,
    ) {
        state.view_start = address_space.clamp_start(jump);
        state.jump_input = address_space.format(state.view_start);
    }

    ui.separator();
    data_inspector::draw_data_inspector(
        ui,
        &mut state.inspector_addr_input,
        &mut state.inspector_addr,
        memory_page,
    );

    ui.separator();
    hex_viewer::draw_tbl_section(ui, &mut state.tbl_map, &mut state.tbl_path);

    writes
}

fn sync_flash_state(state: &mut MemoryViewerState, memory_page: &[(Address, u8)]) {
    if state.flash_ticks.len() != HEX_PAGE_SIZE {
        state.flash_ticks = vec![0; HEX_PAGE_SIZE];
    }

    let page_addr = memory_page.first().map(|(a, _)| *a);
    let same_page = page_addr == Some(state.view_start)
        && state.prev_start == page_addr
        && state.prev_bytes.len() == memory_page.len();

    if same_page {
        state.recent_diffs.clear();
        for (i, (_, value)) in memory_page.iter().enumerate() {
            if *value != state.prev_bytes[i] {
                state.flash_ticks[i] = FLASH_DURATION_TICKS;
                state
                    .recent_diffs
                    .push(crate::debug::types::MemoryByteDiff {
                        address: memory_page[i].0,
                        old: state.prev_bytes[i],
                        new: *value,
                    });
            } else if state.flash_ticks[i] > 0 {
                state.flash_ticks[i] -= 1;
            }
        }
    } else {
        state.recent_diffs.clear();
        for tick in &mut state.flash_ticks {
            *tick = 0;
        }
    }

    state.prev_start = page_addr;
    state.prev_bytes.clear();
    state.prev_bytes.extend(memory_page.iter().map(|(_, v)| *v));
}
