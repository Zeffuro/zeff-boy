use crate::debug::DebugWindowState;

const PAGE_SIZE: usize = 256;
const ROWS_VISIBLE: usize = 16;
const BYTES_PER_ROW: usize = 16;
const MAX_START: u16 = 0xFF00;


pub(super) fn draw_memory_viewer_content(
    ui: &mut egui::Ui,
    state: &mut DebugWindowState,
    memory_page: &[(u16, u8)],
) -> Vec<(u16, u8)> {
    let mut writes = Vec::new();

    sync_flash_state(state, memory_page);

    ui.horizontal(|ui| {
        ui.label("Address:");
        let response = ui.text_edit_singleline(&mut state.memory_jump_input);
        let input_has_focus = response.has_focus();
        let pressed_enter =
            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button("Go").clicked() || pressed_enter {
            if let Some(addr) = parse_u16_hex(&state.memory_jump_input) {
                state.memory_view_start = addr & 0xFFF0;
                state.memory_jump_input = format!("{:04X}", state.memory_view_start);
            }
        }

        if !input_has_focus {
            state.memory_jump_input = format!("{:04X}", state.memory_view_start);
        }
    });

    ui.horizontal(|ui| {
        if ui.button("-0x10").clicked() {
            state.memory_view_start = state.memory_view_start.saturating_sub(0x10);
        }
        if ui.button("+0x10").clicked() {
            state.memory_view_start =
                state.memory_view_start.saturating_add(0x10).min(MAX_START);
        }
        if ui.button("-0x100").clicked() {
            state.memory_view_start = state.memory_view_start.saturating_sub(0x100);
        }
        if ui.button("+0x100").clicked() {
            state.memory_view_start =
                state.memory_view_start.saturating_add(0x100).min(MAX_START);
        }
    });

    if ui.rect_contains_pointer(ui.max_rect()) {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll >= 1.0 {
            state.memory_view_start = state.memory_view_start.saturating_sub(0x10);
        } else if scroll <= -1.0 {
            state.memory_view_start =
                state.memory_view_start.saturating_add(0x10).min(MAX_START);
        }
    }

    let slider = ui.add(
        egui::Slider::new(&mut state.memory_view_start, 0..=MAX_START)
            .step_by(16.0)
            .text("Start"),
    );
    state.memory_view_start &= 0xFFF0;
    if slider.changed() {
        state.memory_jump_input = format!("{:04X}", state.memory_view_start);
    }

    ui.separator();

    let mono = egui::FontId::new(13.0, egui::FontFamily::Monospace);
    let normal_color = ui.visuals().text_color();
    let addr_color = egui::Color32::from_rgb(140, 140, 170);
    let flash_color = egui::Color32::from_rgb(255, 100, 80);
    let dim_color = egui::Color32::from_rgb(90, 90, 90);

    let mut header_job = egui::text::LayoutJob::default();
    header_job.append(
        "Addr   ",
        0.0,
        egui::TextFormat {
            font_id: mono.clone(),
            color: addr_color,
            ..Default::default()
        },
    );
    for i in 0..BYTES_PER_ROW {
        header_job.append(
            &format!("+{:X} ", i),
            0.0,
            egui::TextFormat {
                font_id: mono.clone(),
                color: addr_color,
                ..Default::default()
            },
        );
    }
    header_job.append(
        "  ASCII",
        0.0,
        egui::TextFormat {
            font_id: mono.clone(),
            color: addr_color,
            ..Default::default()
        },
    );
    ui.label(header_job);

    for row in 0..ROWS_VISIBLE {
        let row_start = row * BYTES_PER_ROW;
        if row_start >= memory_page.len() {
            break;
        }
        let row_addr = memory_page[row_start].0;

        let mut job = egui::text::LayoutJob::default();

        job.append(
            &format!("{:04X}:  ", row_addr),
            0.0,
            egui::TextFormat {
                font_id: mono.clone(),
                color: addr_color,
                ..Default::default()
            },
        );

        for col in 0..BYTES_PER_ROW {
            let idx = row_start + col;
            if idx >= memory_page.len() {
                job.append(
                    "-- ",
                    0.0,
                    egui::TextFormat {
                        font_id: mono.clone(),
                        color: dim_color,
                        ..Default::default()
                    },
                );
            } else {
                let (_, value) = memory_page[idx];
                let flash = state.memory_flash_ticks.get(idx).copied().unwrap_or(0);
                let color = if flash > 0 { flash_color } else { normal_color };
                job.append(
                    &format!("{:02X} ", value),
                    0.0,
                    egui::TextFormat {
                        font_id: mono.clone(),
                        color,
                        ..Default::default()
                    },
                );
            }
        }

        job.append(
            "  ",
            0.0,
            egui::TextFormat {
                font_id: mono.clone(),
                color: normal_color,
                ..Default::default()
            },
        );
        for col in 0..BYTES_PER_ROW {
            let idx = row_start + col;
            if idx < memory_page.len() {
                let ch = printable_ascii(memory_page[idx].1);
                let color = if ch == '.' { dim_color } else { normal_color };
                job.append(
                    &ch.to_string(),
                    0.0,
                    egui::TextFormat {
                        font_id: mono.clone(),
                        color,
                        ..Default::default()
                    },
                );
            }
        }

        ui.label(job);
    }

    if state.enable_memory_editing {
        ui.separator();
        if let Some(addr) = state.memory_edit_addr {
            ui.horizontal(|ui| {
                ui.monospace(format!("Edit {:04X}:", addr));
                ui.add(
                    egui::TextEdit::singleline(&mut state.memory_edit_value)
                        .desired_width(50.0)
                        .char_limit(2),
                );
                if ui.button("Write").clicked() {
                    if let Some(value) = parse_u8_hex(&state.memory_edit_value) {
                        writes.push((addr, value));
                    }
                    state.memory_edit_addr = None;
                }
                if ui.button("Cancel").clicked() {
                    state.memory_edit_addr = None;
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label("Edit addr:");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut state.memory_edit_addr_input)
                    .desired_width(60.0)
                    .char_limit(4)
                    .hint_text("hex addr"),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some(addr) = parse_u16_hex(&state.memory_edit_addr_input) {
                    state.memory_edit_addr = Some(addr);
                    let val = memory_page
                        .iter()
                        .find(|(a, _)| *a == addr)
                        .map(|(_, v)| *v)
                        .unwrap_or(0);
                    state.memory_edit_value = format!("{:02X}", val);
                }
            }
        });
    }


    writes
}

fn sync_flash_state(state: &mut DebugWindowState, memory_page: &[(u16, u8)]) {
    let current: Vec<u8> = memory_page.iter().map(|(_, v)| *v).collect();
    if state.memory_flash_ticks.len() != PAGE_SIZE {
        state.memory_flash_ticks = vec![0; PAGE_SIZE];
    }

    let page_addr = memory_page.first().map(|(a, _)| *a);
    let same_page = page_addr == Some(state.memory_view_start)
        && state.memory_prev_start == page_addr
        && state.memory_prev_bytes.len() == current.len();

    if same_page {
        for (i, value) in current.iter().enumerate() {
            if *value != state.memory_prev_bytes[i] {
                state.memory_flash_ticks[i] = 12;
            } else if state.memory_flash_ticks[i] > 0 {
                state.memory_flash_ticks[i] -= 1;
            }
        }
    } else {
        for tick in &mut state.memory_flash_ticks {
            *tick = 0;
        }
    }

    state.memory_prev_start = page_addr;
    state.memory_prev_bytes = current;
}

fn parse_u16_hex(input: &str) -> Option<u16> {
    let trimmed = input.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    u16::from_str_radix(hex, 16)
        .ok()
        .or_else(|| trimmed.parse::<u16>().ok())
}

fn parse_u8_hex(input: &str) -> Option<u8> {
    let trimmed = input.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    u8::from_str_radix(hex, 16)
        .ok()
        .or_else(|| trimmed.parse::<u8>().ok())
}

fn printable_ascii(byte: u8) -> char {
    if (0x20..=0x7E).contains(&byte) {
        byte as char
    } else {
        '.'
    }
}
