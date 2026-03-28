use super::common::{
    parse_hex_u32,
    COLOR_ADDR, COLOR_DIM,
    DEBUG_MONO_FONT_SIZE, HEX_BYTES_PER_ROW, HEX_PAGE_SIZE, HEX_ROWS_VISIBLE,
};
use crate::debug::types::{MemorySearchMode, RomViewerState};

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

    let max_start = rom_size.saturating_sub(HEX_PAGE_SIZE as u32);
    let max_start = max_start & !0xF;

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

    if ui.rect_contains_pointer(ui.max_rect()) {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll >= 1.0 {
            state.view_start = state.view_start.saturating_sub(0x10);
        } else if scroll <= -1.0 {
            state.view_start = state.view_start.saturating_add(0x10).min(max_start);
        }
    }

    let bank = state.view_start / ROM_BANK_SIZE;
    ui.label(format!("Bank: {} (0x{:02X})", bank, bank));

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
    let fmt_dim = egui::TextFormat {
        font_id: mono,
        color: COLOR_DIM,
        ..Default::default()
    };

    let mut header_job = egui::text::LayoutJob::default();
    header_job.append("Offset   ", 0.0, fmt_addr.clone());
    for i in 0..HEX_BYTES_PER_ROW {
        header_job.append(&format!("+{:X} ", i), 0.0, fmt_addr.clone());
    }
    header_job.append("  ASCII", 0.0, fmt_addr.clone());
    ui.label(header_job);

    for row in 0..HEX_ROWS_VISIBLE {
        let row_start = row * HEX_BYTES_PER_ROW;
        if row_start >= rom_page.len() {
            break;
        }
        let row_offset = rom_page[row_start].0;

        let mut job = egui::text::LayoutJob::default();

        job.append(&format!("{:06X}:  ", row_offset), 0.0, fmt_addr.clone());

        for col in 0..HEX_BYTES_PER_ROW {
            let idx = row_start + col;
            if idx >= rom_page.len() {
                job.append("-- ", 0.0, fmt_dim.clone());
            } else {
                let (_, value) = rom_page[idx];
                job.append(&format!("{:02X} ", value), 0.0, fmt_normal.clone());
            }
        }

        job.append("  ", 0.0, fmt_normal.clone());
        for col in 0..HEX_BYTES_PER_ROW {
            let idx = row_start + col;
            if idx < rom_page.len() {
                let byte = rom_page[idx].1;
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

    ui.separator();
    ui.collapsing("🔍 Search ROM", |ui| {
        ui.horizontal(|ui| {
            ui.label("Mode:");
            egui::ComboBox::from_id_salt("rom_search_mode")
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
                MemorySearchMode::AsciiString => "e.g. POKEMON",
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
                        let bank = result.offset / ROM_BANK_SIZE;
                        let label = format!(
                            "{:06X} [bank {:02X}]: {}",
                            result.offset,
                            bank,
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
                            state.view_start = result.offset & !0xF;
                            state.jump_input = format!("{:06X}", state.view_start);
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
}
