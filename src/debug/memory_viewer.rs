use crate::debug::DebugWindowState;

const PAGE_SIZE: usize = 256;
const ROWS_VISIBLE: usize = 16;
const BYTES_PER_ROW: usize = 16;
const MAX_START: u16 = 0xFF00;

pub(crate) fn draw_memory_viewer(
    ctx: &egui::Context,
    state: &mut DebugWindowState,
    memory_page: &[(u16, u8)],
) -> Vec<(u16, u8)> {
    let mut writes = Vec::new();

    sync_flash_state(state, memory_page);

    let mut open = state.show_memory_viewer;
    egui::Window::new("Memory Viewer")
        .open(&mut open)
        .default_width(760.0)
        .show(ctx, |ui| {
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
            ui.monospace("Address   +0 +1 +2 +3 +4 +5 +6 +7 +8 +9 +A +B +C +D +E +F   ASCII");

            for row in 0..ROWS_VISIBLE {
                let row_start = row * BYTES_PER_ROW;
                if row_start >= memory_page.len() {
                    break;
                }
                let row_addr = memory_page[row_start].0;

                ui.horizontal(|ui| {
                    ui.monospace(format!("{:04X}: ", row_addr));
                    for col in 0..BYTES_PER_ROW {
                        let idx = row_start + col;
                        if idx >= memory_page.len() {
                            ui.monospace("--");
                            continue;
                        }
                        let (addr, value) = memory_page[idx];
                        let flash = state.memory_flash_ticks.get(idx).copied().unwrap_or(0);

                        #[cfg(debug_assertions)]
                        {
                            let mut button = egui::Button::new(format!("{:02X}", value));
                            if flash > 0 {
                                button = button.fill(egui::Color32::from_rgb(140, 60, 30));
                            }
                            if ui.add(button).clicked() {
                                state.memory_edit_addr = Some(addr);
                                state.memory_edit_value = format!("{:02X}", value);
                            }
                        }

                        #[cfg(not(debug_assertions))]
                        {
                            if flash > 0 {
                                ui.colored_label(
                                    egui::Color32::LIGHT_RED,
                                    format!("{:02X}", value),
                                );
                            } else {
                                ui.monospace(format!("{:02X}", value));
                            }
                        }
                    }

                    let ascii: String = memory_page
                        [row_start..(row_start + BYTES_PER_ROW).min(memory_page.len())]
                        .iter()
                        .map(|(_, b)| printable_ascii(*b))
                        .collect();
                    ui.monospace(format!("  {}", ascii));
                });
            }

            #[cfg(debug_assertions)]
            {
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
                } else {
                    ui.label("Click a byte to edit (debug builds only).");
                }
            }

            #[cfg(not(debug_assertions))]
            ui.label("Editing is disabled in release builds.");
        });

    state.show_memory_viewer = open;

    writes
}

fn sync_flash_state(state: &mut DebugWindowState, memory_page: &[(u16, u8)]) {
    let current: Vec<u8> = memory_page.iter().map(|(_, v)| *v).collect();
    if state.memory_flash_ticks.len() != PAGE_SIZE {
        state.memory_flash_ticks = vec![0; PAGE_SIZE];
    }

    if state.memory_prev_start == Some(state.memory_view_start)
        && state.memory_prev_bytes.len() == current.len()
    {
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

    state.memory_prev_start = Some(state.memory_view_start);
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
