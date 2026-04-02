use std::collections::HashMap;

use super::common::{
    COLOR_ADDR, COLOR_DIM, COLOR_FLASH, DEBUG_MONO_FONT_SIZE, HEX_BYTES_PER_ROW, HEX_ROWS_VISIBLE,
    parse_hex_u8, parse_hex_u16, parse_hex_u32,
};
use crate::debug::types::{
    MemoryBookmark, MemoryByteDiff, MemorySearchMode, MemorySearchResult, RomSearchResult,
};

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
        addr: egui::TextFormat {
            font_id: mono.clone(),
            color: COLOR_ADDR,
            ..Default::default()
        },
        normal: egui::TextFormat {
            font_id: mono.clone(),
            color: normal_color,
            ..Default::default()
        },
        dim: egui::TextFormat {
            font_id: mono.clone(),
            color: COLOR_DIM,
            ..Default::default()
        },
        flash: egui::TextFormat {
            font_id: mono,
            color: COLOR_FLASH,
            ..Default::default()
        },
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
                let has_flash = flash_ticks.and_then(|ft| ft.get(idx)).copied().unwrap_or(0) > 0;
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

pub(super) fn handle_scroll(
    ui: &mut egui::Ui,
    hover_rect: egui::Rect,
    view_start: u32,
    max_start: u32,
) -> u32 {
    if ui.rect_contains_pointer(hover_rect) {
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
                    ui.selectable_value(mode, MemorySearchMode::ByteSequence, "Sequence (hex)");
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
            ui.add(egui::DragValue::new(max_results).range(1..=1024).speed(1));
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

pub(super) fn draw_bookmarks_section(
    ui: &mut egui::Ui,
    addr_input: &mut String,
    label_input: &mut String,
    bookmarks: &mut Vec<MemoryBookmark>,
    current_view_start: u16,
) -> Option<u16> {
    let mut jump_to = None;
    ui.collapsing("Bookmarks", |ui| {
        ui.horizontal(|ui| {
            ui.label("Address:");
            ui.add(
                egui::TextEdit::singleline(addr_input)
                    .desired_width(60.0)
                    .char_limit(4)
                    .hint_text("hex"),
            );
            if ui.button("Current").clicked() {
                *addr_input = format!("{:04X}", current_view_start);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Label:");
            ui.add(
                egui::TextEdit::singleline(label_input)
                    .desired_width(170.0)
                    .hint_text("optional"),
            );
        });
        ui.horizontal(|ui| {
            if ui.button("Add / Update").clicked()
                && let Some(address) = parse_hex_u16(addr_input)
            {
                upsert_bookmark(bookmarks, address, label_input);
                *addr_input = format!("{:04X}", address);
                label_input.clear();
            }
            if !bookmarks.is_empty() && ui.button("Clear All").clicked() {
                bookmarks.clear();
            }
        });

        if bookmarks.is_empty() {
            ui.label("No bookmarks yet.");
            return;
        }

        egui::ScrollArea::vertical()
            .max_height(150.0)
            .show(ui, |ui| {
                let mut remove_idx = None;
                for (idx, bookmark) in bookmarks.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let row_label = format!("{:04X}  {}", bookmark.address, bookmark.label);
                        if ui
                            .add(
                                egui::Label::new(egui::RichText::new(row_label).monospace())
                                    .sense(egui::Sense::click()),
                            )
                            .clicked()
                        {
                            jump_to = Some(bookmark.address);
                        }
                        if ui.small_button("Jump").clicked() {
                            jump_to = Some(bookmark.address);
                        }
                        if ui.small_button("X").clicked() {
                            remove_idx = Some(idx);
                        }
                    });
                }
                if let Some(idx) = remove_idx {
                    bookmarks.remove(idx);
                }
            });
    });
    jump_to
}

pub(super) fn draw_diff_section(ui: &mut egui::Ui, diffs: &[MemoryByteDiff]) -> Option<u16> {
    let mut jump_to = None;
    ui.collapsing("Diff View", |ui| {
        if diffs.is_empty() {
            ui.label("No byte changes detected on this page yet.");
            return;
        }
        ui.label(format!("{} byte(s) changed:", diffs.len()));
        egui::ScrollArea::vertical()
            .max_height(140.0)
            .show(ui, |ui| {
                for diff in diffs {
                    let line = format_diff_line(*diff);
                    if ui
                        .add(
                            egui::Label::new(egui::RichText::new(line).monospace())
                                .sense(egui::Sense::click()),
                        )
                        .clicked()
                    {
                        jump_to = Some(diff.address);
                    }
                }
            });
    });
    jump_to
}

pub(super) fn draw_pattern_section(
    ui: &mut egui::Ui,
    query: &mut String,
    max_results: &mut usize,
    results: &mut Vec<MemorySearchResult>,
    error: &mut Option<String>,
    memory_page: &[(u16, u8)],
) -> Option<u16> {
    let mut jump_to = None;
    ui.collapsing("Pattern Data", |ui| {
        ui.label("Match hex bytes with optional wildcard `??` (e.g. A9 ?? 00)");
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(query)
                    .desired_width(180.0)
                    .hint_text("A9 ?? 00"),
            );
            let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Find").clicked() || enter_pressed {
                match parse_pattern_query(query) {
                    Ok(pattern) => {
                        *results = find_pattern_matches(memory_page, &pattern, *max_results);
                        *error = None;
                    }
                    Err(e) => {
                        results.clear();
                        *error = Some(e.to_string());
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Max results:");
            ui.add(egui::DragValue::new(max_results).range(1..=512).speed(1));
        });
        if let Some(msg) = error {
            ui.colored_label(egui::Color32::YELLOW, msg);
        }
        if !results.is_empty() {
            ui.label(format!("{} match(es):", results.len()));
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    for result in results {
                        let label = result.display_label();
                        if ui
                            .add(
                                egui::Label::new(egui::RichText::new(label).monospace())
                                    .sense(egui::Sense::click()),
                            )
                            .clicked()
                        {
                            jump_to = Some(result.address);
                        }
                    }
                });
        }
    });
    jump_to
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

pub(super) fn draw_data_inspector(
    ui: &mut egui::Ui,
    addr_input: &mut String,
    inspector_addr: &mut Option<u16>,
    memory_page: &[(u16, u8)],
) {
    ui.collapsing("🔬 Data Inspector", |ui| {
        ui.horizontal(|ui| {
            ui.label("Address:");
            let resp = ui.add(
                egui::TextEdit::singleline(addr_input)
                    .desired_width(60.0)
                    .char_limit(4)
                    .hint_text("hex"),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Inspect").clicked() || enter)
                && let Some(addr) = parse_hex_u16(addr_input)
            {
                *inspector_addr = Some(addr);
            }
            if inspector_addr.is_some() && ui.button("Clear").clicked() {
                *inspector_addr = None;
            }
        });

        let Some(base_addr) = *inspector_addr else {
            ui.label("Enter an address to inspect.");
            return;
        };

        let bytes = read_bytes_at(memory_page, base_addr, 4);
        if bytes.is_empty() {
            ui.label(format!("Address {:04X} not in current page.", base_addr));
            return;
        }

        let mono = egui::FontId::new(DEBUG_MONO_FONT_SIZE, egui::FontFamily::Monospace);
        let label_color = COLOR_ADDR;
        let value_color = ui.visuals().text_color();

        let b0 = bytes[0];
        inspector_row(ui, &mono, label_color, value_color, "u8", &format!("{}", b0));
        inspector_row(ui, &mono, label_color, value_color, "i8", &format!("{}", b0 as i8));
        inspector_row(ui, &mono, label_color, value_color, "Hex", &format!("0x{:02X}", b0));
        inspector_row(ui, &mono, label_color, value_color, "Binary", &format!("{:08b}", b0));

        let ch = if b0.is_ascii_graphic() || b0 == b' ' {
            format!("'{}'", b0 as char)
        } else {
            format!("·  (0x{:02X})", b0)
        };
        inspector_row(ui, &mono, label_color, value_color, "ASCII", &ch);

        if bytes.len() >= 2 {
            let u16le = u16::from_le_bytes([bytes[0], bytes[1]]);
            let u16be = u16::from_be_bytes([bytes[0], bytes[1]]);
            ui.separator();
            inspector_row(ui, &mono, label_color, value_color, "u16 LE", &format!("{}", u16le));
            inspector_row(ui, &mono, label_color, value_color, "u16 BE", &format!("{}", u16be));
            inspector_row(ui, &mono, label_color, value_color, "i16 LE", &format!("{}", u16le as i16));
            inspector_row(ui, &mono, label_color, value_color, "i16 BE", &format!("{}", u16be as i16));
        }

        if bytes.len() >= 4 {
            let u32le = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let u32be = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            ui.separator();
            inspector_row(ui, &mono, label_color, value_color, "u32 LE", &format!("{}", u32le));
            inspector_row(ui, &mono, label_color, value_color, "u32 BE", &format!("{}", u32be));
            inspector_row(ui, &mono, label_color, value_color, "i32 LE", &format!("{}", u32le as i32));
            inspector_row(ui, &mono, label_color, value_color, "i32 BE", &format!("{}", u32be as i32));
        }
    });
}

pub(super) fn draw_data_inspector_rom(
    ui: &mut egui::Ui,
    addr_input: &mut String,
    inspector_addr: &mut Option<u32>,
    rom_page: &[(u32, u8)],
) {
    ui.collapsing("🔬 Data Inspector", |ui| {
        ui.horizontal(|ui| {
            ui.label("Offset:");
            let resp = ui.add(
                egui::TextEdit::singleline(addr_input)
                    .desired_width(80.0)
                    .char_limit(6)
                    .hint_text("hex"),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Inspect").clicked() || enter)
                && let Some(addr) = parse_hex_u32(addr_input)
            {
                *inspector_addr = Some(addr);
            }
            if inspector_addr.is_some() && ui.button("Clear").clicked() {
                *inspector_addr = None;
            }
        });

        let Some(base_addr) = *inspector_addr else {
            ui.label("Enter an offset to inspect.");
            return;
        };

        let bytes = read_bytes_at_u32(rom_page, base_addr, 4);
        if bytes.is_empty() {
            ui.label(format!("Offset {:06X} not in current page.", base_addr));
            return;
        }

        let mono = egui::FontId::new(DEBUG_MONO_FONT_SIZE, egui::FontFamily::Monospace);
        let label_color = COLOR_ADDR;
        let value_color = ui.visuals().text_color();

        let b0 = bytes[0];
        inspector_row(ui, &mono, label_color, value_color, "u8", &format!("{}", b0));
        inspector_row(ui, &mono, label_color, value_color, "i8", &format!("{}", b0 as i8));
        inspector_row(ui, &mono, label_color, value_color, "Hex", &format!("0x{:02X}", b0));
        inspector_row(ui, &mono, label_color, value_color, "Binary", &format!("{:08b}", b0));

        let ch = if b0.is_ascii_graphic() || b0 == b' ' {
            format!("'{}'", b0 as char)
        } else {
            format!("·  (0x{:02X})", b0)
        };
        inspector_row(ui, &mono, label_color, value_color, "ASCII", &ch);

        if bytes.len() >= 2 {
            let u16le = u16::from_le_bytes([bytes[0], bytes[1]]);
            let u16be = u16::from_be_bytes([bytes[0], bytes[1]]);
            ui.separator();
            inspector_row(ui, &mono, label_color, value_color, "u16 LE", &format!("{}", u16le));
            inspector_row(ui, &mono, label_color, value_color, "u16 BE", &format!("{}", u16be));
            inspector_row(ui, &mono, label_color, value_color, "i16 LE", &format!("{}", u16le as i16));
            inspector_row(ui, &mono, label_color, value_color, "i16 BE", &format!("{}", u16be as i16));
        }

        if bytes.len() >= 4 {
            let u32le = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let u32be = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            ui.separator();
            inspector_row(ui, &mono, label_color, value_color, "u32 LE", &format!("{}", u32le));
            inspector_row(ui, &mono, label_color, value_color, "u32 BE", &format!("{}", u32be));
            inspector_row(ui, &mono, label_color, value_color, "i32 LE", &format!("{}", u32le as i32));
            inspector_row(ui, &mono, label_color, value_color, "i32 BE", &format!("{}", u32be as i32));
        }
    });
}

fn inspector_row(
    ui: &mut egui::Ui,
    mono: &egui::FontId,
    label_color: egui::Color32,
    value_color: egui::Color32,
    label: &str,
    value: &str,
) {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &format!("{:<12}", label),
        0.0,
        egui::TextFormat {
            font_id: mono.clone(),
            color: label_color,
            ..Default::default()
        },
    );
    job.append(
        value,
        0.0,
        egui::TextFormat {
            font_id: mono.clone(),
            color: value_color,
            ..Default::default()
        },
    );
    ui.label(job);
}

fn read_bytes_at(page: &[(u16, u8)], base: u16, count: usize) -> Vec<u8> {
    let mut result = Vec::with_capacity(count);
    for offset in 0..count {
        let addr = base.wrapping_add(offset as u16);
        if let Some((_, val)) = page.iter().find(|(a, _)| *a == addr) {
            result.push(*val);
        } else {
            break;
        }
    }
    result
}

fn read_bytes_at_u32(page: &[(u32, u8)], base: u32, count: usize) -> Vec<u8> {
    let mut result = Vec::with_capacity(count);
    for offset in 0..count {
        let addr = base.wrapping_add(offset as u32);
        if let Some((_, val)) = page.iter().find(|(a, _)| *a == addr) {
            result.push(*val);
        } else {
            break;
        }
    }
    result
}

fn upsert_bookmark(bookmarks: &mut Vec<MemoryBookmark>, address: u16, label_input: &str) {
    let label = normalize_bookmark_label(address, label_input);
    if let Some(existing) = bookmarks.iter_mut().find(|entry| entry.address == address) {
        existing.label = label;
    } else {
        bookmarks.push(MemoryBookmark { address, label });
        bookmarks.sort_by_key(|entry| entry.address);
    }
}

fn normalize_bookmark_label(address: u16, label_input: &str) -> String {
    let trimmed = label_input.trim();
    if trimmed.is_empty() {
        format!("0x{address:04X}")
    } else {
        trimmed.to_string()
    }
}

fn format_diff_line(diff: MemoryByteDiff) -> String {
    format!("{:04X}: {:02X} -> {:02X}", diff.address, diff.old, diff.new)
}

fn parse_pattern_query(query: &str) -> Result<Vec<Option<u8>>, &'static str> {
    let mut tokens = Vec::new();
    for token in query.split_whitespace() {
        let normalized = token.trim_start_matches("0x").trim_start_matches("0X");
        if normalized == "??" {
            tokens.push(None);
            continue;
        }
        if normalized.len() != 2 {
            return Err("Pattern tokens must be 2-digit hex bytes or ??.");
        }
        let value = u8::from_str_radix(normalized, 16)
            .map_err(|_| "Pattern tokens must be 2-digit hex bytes or ??.")?;
        tokens.push(Some(value));
    }
    if tokens.is_empty() {
        return Err("Pattern is empty.");
    }
    Ok(tokens)
}

fn find_pattern_matches(
    memory_page: &[(u16, u8)],
    pattern: &[Option<u8>],
    max_results: usize,
) -> Vec<MemorySearchResult> {
    if pattern.is_empty() || memory_page.len() < pattern.len() || max_results == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    for start in 0..=(memory_page.len() - pattern.len()) {
        let mut matched = true;
        for (idx, expected) in pattern.iter().enumerate() {
            if let Some(value) = expected
                && memory_page[start + idx].1 != *value
            {
                matched = false;
                break;
            }
        }
        if matched {
            out.push(MemorySearchResult {
                address: memory_page[start].0,
                matched_bytes: memory_page[start..start + pattern.len()]
                    .iter()
                    .map(|(_, b)| *b)
                    .collect(),
            });
            if out.len() >= max_results {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_bookmark_inserts_sorted_and_dedups_by_address() {
        let mut bookmarks = vec![MemoryBookmark {
            address: 0xC100,
            label: "A".to_string(),
        }];
        upsert_bookmark(&mut bookmarks, 0xC000, "Start");
        upsert_bookmark(&mut bookmarks, 0xC100, "Renamed");
        assert_eq!(bookmarks.len(), 2);
        assert_eq!(bookmarks[0].address, 0xC000);
        assert_eq!(bookmarks[0].label, "Start");
        assert_eq!(bookmarks[1].address, 0xC100);
        assert_eq!(bookmarks[1].label, "Renamed");
    }

    #[test]
    fn normalize_bookmark_label_falls_back_to_hex_address() {
        assert_eq!(normalize_bookmark_label(0xC000, "   "), "0xC000");
    }

    #[test]
    fn format_diff_line_has_expected_layout() {
        let line = format_diff_line(MemoryByteDiff {
            address: 0xC123,
            old: 0x1A,
            new: 0x2B,
        });
        assert_eq!(line, "C123: 1A -> 2B");
    }

    #[test]
    fn parse_pattern_query_supports_wildcards() {
        let parsed = parse_pattern_query("A9 ?? 00").unwrap();
        assert_eq!(parsed, vec![Some(0xA9), None, Some(0x00)]);
    }

    #[test]
    fn parse_pattern_query_rejects_invalid_token() {
        assert!(parse_pattern_query("A9 ZZ 00").is_err());
    }

    #[test]
    fn find_pattern_matches_handles_wildcards() {
        let page = vec![
            (0xC000, 0xA9),
            (0xC001, 0x01),
            (0xC002, 0x00),
            (0xC003, 0xA9),
            (0xC004, 0xFF),
            (0xC005, 0x00),
        ];
        let matches = find_pattern_matches(&page, &[Some(0xA9), None, Some(0x00)], 16);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].address, 0xC000);
        assert_eq!(matches[1].address, 0xC003);
    }

    #[test]
    fn read_bytes_at_u32_reads_partial_tail() {
        let page = vec![(0x123450, 0xAA), (0x123451, 0xBB), (0x123452, 0xCC)];
        let bytes = read_bytes_at_u32(&page, 0x123451, 4);
        assert_eq!(bytes, vec![0xBB, 0xCC]);
    }

    #[test]
    fn read_bytes_at_u32_returns_empty_for_missing_base() {
        let page = vec![(0x10, 0x01), (0x11, 0x02)];
        let bytes = read_bytes_at_u32(&page, 0x20, 4);
        assert!(bytes.is_empty());
    }
}

