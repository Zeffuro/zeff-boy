use std::collections::HashMap;

use super::common::{
    parse_hex_u8,
    COLOR_ADDR, COLOR_DIM, COLOR_FLASH,
    DEBUG_MONO_FONT_SIZE, HEX_BYTES_PER_ROW, HEX_ROWS_VISIBLE,
};
use crate::debug::types::{MemorySearchMode, MemorySearchResult, RomSearchResult};

pub(super) struct HexFormats {
    pub addr: egui::TextFormat,
    pub normal: egui::TextFormat,
    pub dim: egui::TextFormat,
    pub flash: egui::TextFormat,
}

pub(super) fn hex_text_formats(ui: &egui::Ui) -> HexFormats {
    let mono = egui::FontId::new(DEBUG_MONO_FONT_SIZE, egui::FontFamily::Monospace);
    let normal_color = ui.visuals().text_color();
    HexFormats {
        addr: egui::TextFormat { font_id: mono.clone(), color: COLOR_ADDR, ..Default::default() },
        normal: egui::TextFormat { font_id: mono.clone(), color: normal_color, ..Default::default() },
        dim: egui::TextFormat { font_id: mono.clone(), color: COLOR_DIM, ..Default::default() },
        flash: egui::TextFormat { font_id: mono, color: COLOR_FLASH, ..Default::default() },
    }
}

pub(super) fn draw_hex_header(ui: &mut egui::Ui, addr_label: &str, fmt: &HexFormats) {
    let mut job = egui::text::LayoutJob::default();
    job.append(addr_label, 0.0, fmt.addr.clone());
    for i in 0..HEX_BYTES_PER_ROW {
        job.append(&format!("+{:X} ", i), 0.0, fmt.addr.clone());
    }
    job.append("  ASCII", 0.0, fmt.addr.clone());
    ui.label(job);
}

pub(super) fn draw_hex_grid<A: Copy + Into<u32>>(
    ui: &mut egui::Ui,
    page: &[(A, u8)],
    addr_width: usize,
    fmt: &HexFormats,
    flash_ticks: Option<&[u8]>,
    tbl_map: &HashMap<u8, String>,
) {
    for row in 0..HEX_ROWS_VISIBLE {
        let row_start = row * HEX_BYTES_PER_ROW;
        if row_start >= page.len() {
            break;
        }
        let row_addr: u32 = page[row_start].0.into();

        let mut job = egui::text::LayoutJob::default();
        match addr_width {
            4 => job.append(&format!("{:04X}:  ", row_addr), 0.0, fmt.addr.clone()),
            6 => job.append(&format!("{:06X}:  ", row_addr), 0.0, fmt.addr.clone()),
            _ => job.append(&format!("{:08X}:  ", row_addr), 0.0, fmt.addr.clone()),
        }

        for col in 0..HEX_BYTES_PER_ROW {
            let idx = row_start + col;
            if idx >= page.len() {
                job.append("-- ", 0.0, fmt.dim.clone());
            } else {
                let value = page[idx].1;
                let has_flash = flash_ticks
                    .and_then(|ft| ft.get(idx))
                    .copied()
                    .unwrap_or(0)
                    > 0;
                let text_fmt = if has_flash { &fmt.flash } else { &fmt.normal };
                job.append(&format!("{:02X} ", value), 0.0, text_fmt.clone());
            }
        }

        job.append("  ", 0.0, fmt.normal.clone());
        for col in 0..HEX_BYTES_PER_ROW {
            let idx = row_start + col;
            if idx < page.len() {
                let byte = page[idx].1;
                let (ch, is_mapped) = super::common::tbl_lookup(byte, tbl_map);
                let text_fmt = if !is_mapped && ch == "." {
                    &fmt.dim
                } else {
                    &fmt.normal
                };
                job.append(&ch, 0.0, text_fmt.clone());
            }
        }

        ui.label(job);
    }
}

pub(super) fn handle_scroll(ui: &mut egui::Ui, view_start: u32, max_start: u32) -> u32 {
    if ui.rect_contains_pointer(ui.max_rect()) {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll >= 1.0 {
            return view_start.saturating_sub(0x10);
        } else if scroll <= -1.0 {
            return view_start.saturating_add(0x10).min(max_start);
        }
    }
    view_start
}

pub(super) trait HexSearchResult {
    fn display_label(&self) -> String;
    fn jump_address(&self) -> u32;
}

impl HexSearchResult for MemorySearchResult {
    fn display_label(&self) -> String {
        format!(
            "{:04X}: {}",
            self.address,
            self.matched_bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
    fn jump_address(&self) -> u32 {
        self.address as u32
    }
}

impl HexSearchResult for RomSearchResult {
    fn display_label(&self) -> String {
        let bank = self.offset / 0x4000;
        format!(
            "{:06X} [bank {:02X}]: {}",
            self.offset,
            bank,
            self.matched_bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
    fn jump_address(&self) -> u32 {
        self.offset
    }
}

pub(super) fn draw_search_section<R: HexSearchResult>(
    ui: &mut egui::Ui,
    heading: &str,
    id_salt: &str,
    mode: &mut MemorySearchMode,
    query: &mut String,
    max_results: &mut usize,
    pending: &mut bool,
    results: &[R],
) -> Option<u32> {
    let mut jump_to = None;
    ui.collapsing(heading, |ui| {
        ui.horizontal(|ui| {
            ui.label("Mode:");
            egui::ComboBox::from_id_salt(id_salt)
                .selected_text(match *mode {
                    MemorySearchMode::ByteValue => "Byte (hex)",
                    MemorySearchMode::ByteSequence => "Sequence (hex)",
                    MemorySearchMode::AsciiString => "ASCII",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(mode, MemorySearchMode::ByteValue, "Byte (hex)");
                    ui.selectable_value(
                        mode,
                        MemorySearchMode::ByteSequence,
                        "Sequence (hex)",
                    );
                    ui.selectable_value(mode, MemorySearchMode::AsciiString, "ASCII");
                });
        });
        ui.horizontal(|ui| {
            let hint = match *mode {
                MemorySearchMode::ByteValue => "e.g. FF",
                MemorySearchMode::ByteSequence => "e.g. FF 00 AB",
                MemorySearchMode::AsciiString => "e.g. HELLO",
            };
            let resp = ui.add(
                egui::TextEdit::singleline(query)
                    .desired_width(150.0)
                    .hint_text(hint),
            );
            let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Search").clicked() || enter_pressed {
                *pending = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Max results:");
            ui.add(
                egui::DragValue::new(max_results)
                    .range(1..=1024)
                    .speed(1),
            );
        });
        if !results.is_empty() {
            ui.label(format!("{} result(s):", results.len()));
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    for result in results {
                        let label = result.display_label();
                        if ui
                            .add(
                                egui::Label::new(egui::RichText::new(&label).monospace())
                                    .sense(egui::Sense::click()),
                            )
                            .clicked()
                        {
                            jump_to = Some(result.jump_address());
                        }
                    }
                });
        }
    });
    jump_to
}

pub(super) fn draw_tbl_section(
    ui: &mut egui::Ui,
    tbl_map: &mut HashMap<u8, String>,
    tbl_path: &mut Option<String>,
) {
    ui.collapsing("TBL Character Map", |ui| {
        if let Some(ref path) = *tbl_path {
            ui.label(format!("Loaded: {}", path));
            if ui.button("Clear TBL").clicked() {
                tbl_map.clear();
                *tbl_path = None;
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
                        *tbl_map = map;
                        *tbl_path = Some(name);
                    }
                    Err(e) => {
                        log::warn!("Failed to load TBL file: {}", e);
                    }
                }
            }
    });
}

pub(crate) fn parse_search_query(query: &str, mode: MemorySearchMode) -> Option<Vec<u8>> {
    match mode {
        MemorySearchMode::ByteValue => parse_hex_u8(query).map(|b| vec![b]),
        MemorySearchMode::ByteSequence => {
            let bytes: Vec<u8> = query
                .split_whitespace()
                .filter_map(|s| {
                    u8::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16)
                        .ok()
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

