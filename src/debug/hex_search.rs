use super::common::parse_hex_u8;
use crate::debug::types::{MemorySearchMode, MemorySearchResult, RomSearchResult};
use std::fmt::Write;

pub(super) trait HexSearchResult {
    fn display_label(&self) -> String;
    fn jump_address(&self) -> u32;
}

fn format_matched_bytes(s: &mut String, bytes: &[u8]) {
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{:02X}", b);
    }
}

impl HexSearchResult for MemorySearchResult {
    fn display_label(&self) -> String {
        let mut s = format!("{:04X}: ", self.address);
        format_matched_bytes(&mut s, &self.matched_bytes);
        s
    }
    fn jump_address(&self) -> u32 {
        self.address as u32
    }
}

impl HexSearchResult for RomSearchResult {
    fn display_label(&self) -> String {
        let bank = self.offset / 0x4000;
        let mut s = format!("{:06X} [bank {:02X}]: ", self.offset, bank);
        format_matched_bytes(&mut s, &self.matched_bytes);
        s
    }
    fn jump_address(&self) -> u32 {
        self.offset
    }
}

pub(super) struct SearchSectionParams<'a> {
    pub mode: &'a mut MemorySearchMode,
    pub query: &'a mut String,
    pub max_results: &'a mut usize,
    pub pending: &'a mut bool,
}

pub(super) fn draw_search_section<R: HexSearchResult>(
    ui: &mut egui::Ui,
    heading: &str,
    id_salt: &str,
    params: &mut SearchSectionParams<'_>,
    results: &[R],
) -> Option<u32> {
    let mut jump_to = None;
    ui.collapsing(heading, |ui| {
        ui.horizontal(|ui| {
            ui.label("Mode:");
            egui::ComboBox::from_id_salt(id_salt)
                .selected_text(match *params.mode {
                    MemorySearchMode::ByteValue => "Byte (hex)",
                    MemorySearchMode::ByteSequence => "Sequence (hex)",
                    MemorySearchMode::AsciiString => "ASCII",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(params.mode, MemorySearchMode::ByteValue, "Byte (hex)");
                    ui.selectable_value(
                        params.mode,
                        MemorySearchMode::ByteSequence,
                        "Sequence (hex)",
                    );
                    ui.selectable_value(params.mode, MemorySearchMode::AsciiString, "ASCII");
                });
        });
        ui.horizontal(|ui| {
            let hint = match *params.mode {
                MemorySearchMode::ByteValue => "e.g. FF",
                MemorySearchMode::ByteSequence => "e.g. FF 00 AB",
                MemorySearchMode::AsciiString => "e.g. HELLO",
            };
            let resp = ui.add(
                egui::TextEdit::singleline(params.query)
                    .desired_width(150.0)
                    .hint_text(hint),
            );
            let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Search").clicked() || enter_pressed {
                *params.pending = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Max results:");
            ui.add(
                egui::DragValue::new(params.max_results)
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
}
