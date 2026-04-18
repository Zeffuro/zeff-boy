use std::collections::HashMap;
use std::fmt::Write;

use super::common::{
    COLOR_ADDR, COLOR_DIM, COLOR_FLASH, DEBUG_MONO_FONT_SIZE, HEX_BYTES_PER_ROW, HEX_ROWS_VISIBLE,
    parse_hex_u16,
};
use crate::debug::types::{MemoryBookmark, MemoryByteDiff};

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
    let mut scratch = String::with_capacity(8);
    job.append(addr_label, 0.0, fmt.addr.clone());
    for i in 0..HEX_BYTES_PER_ROW {
        scratch.clear();
        let _ = write!(scratch, "+{:X} ", i);
        job.append(&scratch, 0.0, fmt.addr.clone());
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
    let mut scratch = String::with_capacity(12);
    for row in 0..HEX_ROWS_VISIBLE {
        let row_start = row * HEX_BYTES_PER_ROW;
        if row_start >= page.len() {
            break;
        }
        let row_addr: u32 = page[row_start].0.into();

        let mut job = egui::text::LayoutJob::default();
        scratch.clear();
        match addr_width {
            4 => { let _ = write!(scratch, "{:04X}:  ", row_addr); }
            6 => { let _ = write!(scratch, "{:06X}:  ", row_addr); }
            _ => { let _ = write!(scratch, "{:08X}:  ", row_addr); }
        }
        job.append(&scratch, 0.0, fmt.addr.clone());

        for col in 0..HEX_BYTES_PER_ROW {
            let idx = row_start + col;
            if idx >= page.len() {
                job.append("-- ", 0.0, fmt.dim.clone());
            } else {
                let value = page[idx].1;
                let has_flash = flash_ticks.and_then(|ft| ft.get(idx)).copied().unwrap_or(0) > 0;
                let text_fmt = if has_flash { &fmt.flash } else { &fmt.normal };
                scratch.clear();
                let _ = write!(scratch, "{:02X} ", value);
                job.append(&scratch, 0.0, text_fmt.clone());
            }
        }

        job.append("  ", 0.0, fmt.normal.clone());
        for col in 0..HEX_BYTES_PER_ROW {
            let idx = row_start + col;
            if idx < page.len() {
                let byte = page[idx].1;
                let ch = super::common::tbl_lookup(byte, tbl_map);
                let text_fmt = if ch.len() == 1 && ch.as_bytes()[0] == b'.' && !tbl_map.contains_key(&byte) {
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
        let scroll = ui.input(| i | i.smooth_scroll_delta.y);
        if scroll >= 1.0 {
            return view_start.saturating_sub(0x10);
        } else if scroll <= -1.0 {
            return view_start.saturating_add(0x10).min(max_start);
        }
    }
    view_start
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
            && let Some(path) = crate::platform::FileDialog::new()
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
}
