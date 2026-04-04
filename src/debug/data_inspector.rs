use super::common::{COLOR_ADDR, DEBUG_MONO_FONT_SIZE, parse_hex_u16, parse_hex_u32};

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

        draw_inspector_body(ui, &bytes);
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

        draw_inspector_body(ui, &bytes);
    });
}

fn draw_inspector_body(ui: &mut egui::Ui, bytes: &[u8]) {
    let mono = egui::FontId::new(DEBUG_MONO_FONT_SIZE, egui::FontFamily::Monospace);
    let label_color = COLOR_ADDR;
    let value_color = ui.visuals().text_color();

    let b0 = bytes[0];
    inspector_row(
        ui,
        &mono,
        label_color,
        value_color,
        "u8",
        &format!("{}", b0),
    );
    inspector_row(
        ui,
        &mono,
        label_color,
        value_color,
        "i8",
        &format!("{}", b0 as i8),
    );
    inspector_row(
        ui,
        &mono,
        label_color,
        value_color,
        "Hex",
        &format!("0x{:02X}", b0),
    );
    inspector_row(
        ui,
        &mono,
        label_color,
        value_color,
        "Binary",
        &format!("{:08b}", b0),
    );

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
        inspector_row(
            ui,
            &mono,
            label_color,
            value_color,
            "u16 LE",
            &format!("{}", u16le),
        );
        inspector_row(
            ui,
            &mono,
            label_color,
            value_color,
            "u16 BE",
            &format!("{}", u16be),
        );
        inspector_row(
            ui,
            &mono,
            label_color,
            value_color,
            "i16 LE",
            &format!("{}", u16le as i16),
        );
        inspector_row(
            ui,
            &mono,
            label_color,
            value_color,
            "i16 BE",
            &format!("{}", u16be as i16),
        );
    }

    if bytes.len() >= 4 {
        let u32le = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let u32be = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        ui.separator();
        inspector_row(
            ui,
            &mono,
            label_color,
            value_color,
            "u32 LE",
            &format!("{}", u32le),
        );
        inspector_row(
            ui,
            &mono,
            label_color,
            value_color,
            "u32 BE",
            &format!("{}", u32be),
        );
        inspector_row(
            ui,
            &mono,
            label_color,
            value_color,
            "i32 LE",
            &format!("{}", u32le as i32),
        );
        inspector_row(
            ui,
            &mono,
            label_color,
            value_color,
            "i32 BE",
            &format!("{}", u32be as i32),
        );
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
