use super::common::{
    parse_hex_u8, parse_hex_u16,
    COLOR_ADDR, COLOR_DIM, COLOR_FLASH,
    DEBUG_MONO_FONT_SIZE, HEX_BYTES_PER_ROW, HEX_PAGE_SIZE, HEX_ROWS_VISIBLE,
};
use crate::debug::types::{MemorySearchMode, MemoryViewerState};

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

    if ui.rect_contains_pointer(ui.max_rect()) {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll >= 1.0 {
            state.view_start = state.view_start.saturating_sub(0x10);
        } else if scroll <= -1.0 {
            state.view_start = state.view_start.saturating_add(0x10).min(MAX_START);
        }
    }

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

    let mono = egui::FontId::new(DEBUG_MONO_FONT_SIZE, egui::FontFamily::Monospace);
    let normal_color = ui.visuals().text_color();

    let fmt_addr = egui::TextFormat {
        font_id: mono.clone(),
        color: COLOR_ADDR,
        ..Default::default()
    };
    let fmt_normal = egui::TextFormat {
        font_id: mono.clone(),
        color: normal_color,
        ..Default::default()
    };
    let fmt_flash = egui::TextFormat {
        font_id: mono.clone(),
        color: COLOR_FLASH,
        ..Default::default()
    };
    let fmt_dim = egui::TextFormat {
        font_id: mono,
        color: COLOR_DIM,
        ..Default::default()
    };

    let mut header_job = egui::text::LayoutJob::default();
    header_job.append("Addr   ", 0.0, fmt_addr.clone());
    for i in 0..HEX_BYTES_PER_ROW {
        header_job.append(&format!("+{:X} ", i), 0.0, fmt_addr.clone());
    }
    header_job.append("  ASCII", 0.0, fmt_addr.clone());
    ui.label(header_job);

    for row in 0..HEX_ROWS_VISIBLE {
        let row_start = row * HEX_BYTES_PER_ROW;
        if row_start >= memory_page.len() {
            break;
        }
        let row_addr = memory_page[row_start].0;

        let mut job = egui::text::LayoutJob::default();

        job.append(&format!("{:04X}:  ", row_addr), 0.0, fmt_addr.clone());

        for col in 0..HEX_BYTES_PER_ROW {
            let idx = row_start + col;
            if idx >= memory_page.len() {
                job.append("-- ", 0.0, fmt_dim.clone());
            } else {
                let (_, value) = memory_page[idx];
                let flash = state.flash_ticks.get(idx).copied().unwrap_or(0);
                let fmt = if flash > 0 { &fmt_flash } else { &fmt_normal };
                job.append(&format!("{:02X} ", value), 0.0, fmt.clone());
            }
        }

        job.append("  ", 0.0, fmt_normal.clone());
        for col in 0..HEX_BYTES_PER_ROW {
            let idx = row_start + col;
            if idx < memory_page.len() {
                let byte = memory_page[idx].1;
                let (ch, is_mapped) = super::common::tbl_lookup(byte, &state.tbl_map);
                let fmt = if is_mapped {
                    &fmt_normal
                } else if ch == "." {
                    &fmt_dim
                } else {
                    &fmt_normal
                };
                job.append(&ch, 0.0, fmt.clone());
            }
        }

        ui.label(job);
    }

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
    ui.collapsing("🔍 Search Memory", |ui| {
        ui.horizontal(|ui| {
            ui.label("Mode:");
            egui::ComboBox::from_id_salt("search_mode")
                .selected_text(match state.search_mode {
                    MemorySearchMode::ByteValue => "Byte (hex)",
                    MemorySearchMode::ByteSequence => "Sequence (hex)",
                    MemorySearchMode::AsciiString => "ASCII",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut state.search_mode,
                        MemorySearchMode::ByteValue,
                        "Byte (hex)",
                    );
                    ui.selectable_value(
                        &mut state.search_mode,
                        MemorySearchMode::ByteSequence,
                        "Sequence (hex)",
                    );
                    ui.selectable_value(
                        &mut state.search_mode,
                        MemorySearchMode::AsciiString,
                        "ASCII",
                    );
                });
        });
        ui.horizontal(|ui| {
            let hint = match state.search_mode {
                MemorySearchMode::ByteValue => "e.g. FF",
                MemorySearchMode::ByteSequence => "e.g. FF 00 AB",
                MemorySearchMode::AsciiString => "e.g. HELLO",
            };
            let resp = ui.add(
                egui::TextEdit::singleline(&mut state.search_query)
                    .desired_width(150.0)
                    .hint_text(hint),
            );
            let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Search").clicked() || enter_pressed {
                state.search_pending = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Max results:");
            ui.add(
                egui::DragValue::new(&mut state.search_max_results)
                    .range(1..=1024)
                    .speed(1),
            );
        });
        if !state.search_results.is_empty() {
            ui.label(format!("{} result(s):", state.search_results.len()));
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    for result in &state.search_results {
                        let label = format!(
                            "{:04X}: {}",
                            result.address,
                            result
                                .matched_bytes
                                .iter()
                                .map(|b| format!("{:02X}", b))
                                .collect::<Vec<_>>()
                                .join(" "),
                        );
                        if ui
                            .add(
                                egui::Label::new(egui::RichText::new(&label).monospace())
                                    .sense(egui::Sense::click()),
                            )
                            .clicked()
                        {
                            state.view_start = result.address & 0xFFF0;
                            state.jump_input = format!("{:04X}", state.view_start);
                        }
                    }
                });
        }
    });

    ui.separator();
    ui.collapsing("TBL Character Map", |ui| {
        if let Some(ref path) = state.tbl_path {
            ui.label(format!("Loaded: {}", path));
            if ui.button("Clear TBL").clicked() {
                state.tbl_map.clear();
                state.tbl_path = None;
            }
        } else {
            ui.label("No TBL file loaded (using ASCII)");
        }
        if ui.button("Load TBL File...").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("TBL files", &["tbl", "txt"])
                .pick_file()
            {
                match super::common::load_tbl_file(&path) {
                    Ok(map) => {
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("?")
                            .to_string();
                        state.tbl_map = map;
                        state.tbl_path = Some(name);
                    }
                    Err(e) => {
                        log::warn!("Failed to load TBL file: {}", e);
                    }
                }
            }
    });

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


pub(crate) fn parse_search_query(query: &str, mode: MemorySearchMode) -> Option<Vec<u8>> {
    match mode {
        MemorySearchMode::ByteValue => parse_hex_u8(query).map(|b| vec![b]),
        MemorySearchMode::ByteSequence => {
            let bytes: Vec<u8> = query
                .split_whitespace()
                .filter_map(|s| {
                    u8::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
                })
                .collect();
            if bytes.is_empty() { None } else { Some(bytes) }
        }
        MemorySearchMode::AsciiString => {
            let bytes: Vec<u8> = query.bytes().collect();
            if bytes.is_empty() { None } else { Some(bytes) }
        }
    }
}
