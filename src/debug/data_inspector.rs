use super::common::{COLOR_ADDR, DEBUG_MONO_FONT_SIZE, parse_hex_u16, parse_hex_u32};

struct InspectorConfig {
    label: &'static str,
    desired_width: f32,
    char_limit: usize,
    empty_hint: &'static str,
    missing_fmt: &'static str,
}

const MEMORY_CONFIG: InspectorConfig = InspectorConfig {
    label: "Address:",
    desired_width: 60.0,
    char_limit: 4,
    empty_hint: "Enter an address to inspect.",
    missing_fmt: "not in current page.",
};

const ROM_CONFIG: InspectorConfig = InspectorConfig {
    label: "Offset:",
    desired_width: 80.0,
    char_limit: 6,
    empty_hint: "Enter an offset to inspect.",
    missing_fmt: "not in current page.",
};

struct InspectorState<'a, A> {
    addr_input: &'a mut String,
    inspector_addr: &'a mut Option<A>,
    page: &'a [(A, u8)],
}

fn draw_inspector_generic<A>(
    ui: &mut egui::Ui,
    state: InspectorState<'_, A>,
    config: &InspectorConfig,
    parse_fn: fn(&str) -> Option<A>,
    format_fn: fn(A) -> String,
) where
    A: Copy + Eq + std::ops::Add<Output = A> + TryFrom<usize> + std::fmt::Display,
{
    ui.collapsing("🔬 Data Inspector", |ui| {
        ui.horizontal(|ui| {
            ui.label(config.label);
            let resp = ui.add(
                egui::TextEdit::singleline(state.addr_input)
                    .desired_width(config.desired_width)
                    .char_limit(config.char_limit)
                    .hint_text("hex"),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Inspect").clicked() || enter)
                && let Some(addr) = parse_fn(state.addr_input)
            {
                *state.inspector_addr = Some(addr);
            }
            if state.inspector_addr.is_some() && ui.button("Clear").clicked() {
                *state.inspector_addr = None;
            }
        });

        let Some(base_addr) = *state.inspector_addr else {
            ui.label(config.empty_hint);
            return;
        };

        let bytes = read_bytes_at(state.page, base_addr, 4);
        if bytes.is_empty() {
            ui.label(format!("{} {}", format_fn(base_addr), config.missing_fmt));
            return;
        }

        draw_inspector_body(ui, &bytes);
    });
}

pub(super) fn draw_data_inspector(
    ui: &mut egui::Ui,
    addr_input: &mut String,
    inspector_addr: &mut Option<u16>,
    memory_page: &[(u16, u8)],
) {
    draw_inspector_generic(
        ui,
        InspectorState {
            addr_input,
            inspector_addr,
            page: memory_page,
        },
        &MEMORY_CONFIG,
        parse_hex_u16,
        |a| format!("Address {:04X}", a),
    );
}

pub(super) fn draw_data_inspector_rom(
    ui: &mut egui::Ui,
    addr_input: &mut String,
    inspector_addr: &mut Option<u32>,
    rom_page: &[(u32, u8)],
) {
    draw_inspector_generic(
        ui,
        InspectorState {
            addr_input,
            inspector_addr,
            page: rom_page,
        },
        &ROM_CONFIG,
        parse_hex_u32,
        |a| format!("Offset {:06X}", a),
    );
}

fn draw_inspector_body(ui: &mut egui::Ui, bytes: &[u8]) {
    let mono = egui::FontId::new(DEBUG_MONO_FONT_SIZE, egui::FontFamily::Monospace);
    let label_color = COLOR_ADDR;
    let value_color = ui.visuals().text_color();

    let b0 = bytes[0];
    let ch = if b0.is_ascii_graphic() || b0 == b' ' {
        format!("'{}'", b0 as char)
    } else {
        format!("·  (0x{:02X})", b0)
    };

    let rows: &[(&str, String)] = &[
        ("u8", format!("{}", b0)),
        ("i8", format!("{}", b0 as i8)),
        ("Hex", format!("0x{:02X}", b0)),
        ("Binary", format!("{:08b}", b0)),
        ("ASCII", ch),
    ];
    for (label, value) in rows {
        inspector_row(ui, &mono, label_color, value_color, label, value);
    }

    if bytes.len() >= 2 {
        let u16le = u16::from_le_bytes([bytes[0], bytes[1]]);
        let u16be = u16::from_be_bytes([bytes[0], bytes[1]]);
        ui.separator();
        let rows: &[(&str, String)] = &[
            ("u16 LE", format!("{}", u16le)),
            ("u16 BE", format!("{}", u16be)),
            ("i16 LE", format!("{}", u16le as i16)),
            ("i16 BE", format!("{}", u16be as i16)),
        ];
        for (label, value) in rows {
            inspector_row(ui, &mono, label_color, value_color, label, value);
        }
    }

    if bytes.len() >= 4 {
        let u32le = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let u32be = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        ui.separator();
        let rows: &[(&str, String)] = &[
            ("u32 LE", format!("{}", u32le)),
            ("u32 BE", format!("{}", u32be)),
            ("i32 LE", format!("{}", u32le as i32)),
            ("i32 BE", format!("{}", u32be as i32)),
        ];
        for (label, value) in rows {
            inspector_row(ui, &mono, label_color, value_color, label, value);
        }
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

fn read_bytes_at<A>(page: &[(A, u8)], base: A, count: usize) -> Vec<u8>
where
    A: Copy + Eq + std::ops::Add<Output = A> + TryFrom<usize>,
{
    let mut result = Vec::with_capacity(count);
    for offset in 0..count {
        let Ok(off) = A::try_from(offset) else { break };
        let addr = base + off;
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
        let page = vec![(0x123450u32, 0xAA), (0x123451, 0xBB), (0x123452, 0xCC)];
        let bytes = read_bytes_at(&page, 0x123451u32, 4);
        assert_eq!(bytes, vec![0xBB, 0xCC]);
    }

    #[test]
    fn read_bytes_at_u32_returns_empty_for_missing_base() {
        let page = vec![(0x10u32, 0x01), (0x11, 0x02)];
        let bytes = read_bytes_at(&page, 0x20u32, 4);
        assert!(bytes.is_empty());
    }
}
