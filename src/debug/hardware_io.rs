use crate::debug::common::{color32, debug_colors, debug_mono_font};
use crate::debug::types::CpuDebugViewState;
use crate::debug::{CpuDebugSnapshot, DebugTab, DebugUiActions, IoRegisterDisplay};

pub(super) fn draw_hardware_io_content(
    ui: &mut egui::Ui,
    info: &CpuDebugSnapshot,
    state: &mut CpuDebugViewState,
    actions: &mut DebugUiActions,
) {
    state.sync(info);
    if info.sections.is_empty() {
        ui.label("No hardware register snapshot available.");
        return;
    }

    let colors = debug_colors(ui);
    let mono = debug_mono_font(ui);
    ui.horizontal_wrapped(|ui| {
        ui.weak("Live hardware state");
        ui.colored_label(color32(colors.changed), "changed");
        ui.colored_label(color32(colors.symbol), "active");
        ui.colored_label(color32(colors.interrupt), "interrupt");
    });

    draw_decoded_registers(ui, info, actions);

    for (section_index, section) in info.sections.iter().enumerate() {
        let section_color = section_color(ui, section.heading);
        egui::CollapsingHeader::new(
            egui::RichText::new(section.heading)
                .color(section_color)
                .strong(),
        )
        .id_salt(("hardware_io_section", section.heading))
        .default_open(true)
        .show(ui, |ui| {
            for (line_index, line) in section.lines.iter().enumerate() {
                let changed = state.section_line_changed(section_index, line_index);
                ui.label(semantic_line(ui, line, &mono, section_color, changed));
            }
        });
    }
}

fn draw_decoded_registers(
    ui: &mut egui::Ui,
    info: &CpuDebugSnapshot,
    actions: &mut DebugUiActions,
) {
    if info.io_registers.is_empty() {
        return;
    }
    let colors = debug_colors(ui);
    let mono = debug_mono_font(ui);
    let suspended = info.cpu_state.eq_ignore_ascii_case("Suspended");
    ui.separator();
    ui.strong("Decoded registers");
    if !suspended {
        ui.weak("Pause to edit writable bits.");
    }
    for register in &info.io_registers {
        ui.horizontal_wrapped(|ui| {
            let address = ui.small_button(
                egui::RichText::new(format_address(register))
                    .font(mono.clone())
                    .color(color32(colors.address)),
            );
            if address.clicked() {
                actions.memory_target = Some(register.address);
                actions.focus_tab = Some(DebugTab::MemoryViewer);
            }
            address.on_hover_text("Open in Memory Viewer");
            ui.label(
                egui::RichText::new(register.name)
                    .font(mono.clone())
                    .color(color32(colors.symbol))
                    .strong(),
            );
            ui.label(
                egui::RichText::new(format_value(register))
                    .font(mono.clone())
                    .color(color32(colors.opcode)),
            );
            for bit in &register.bits {
                let set = register.value & bit.mask != 0;
                let writable = register.writable_mask & bit.mask != 0;
                let color = if set {
                    color32(colors.symbol)
                } else {
                    ui.visuals().weak_text_color()
                };
                let response = ui.add_enabled(
                    suspended && writable,
                    egui::Button::new(egui::RichText::new(bit.label).color(color)).selected(set),
                );
                if response.clicked() {
                    write_register(actions, register, register.value ^ bit.mask);
                }
                if !writable {
                    response.on_hover_text("Read-only");
                } else if suspended {
                    response.on_hover_text("Click to toggle");
                }
            }
        });
    }
    ui.separator();
}

fn write_register(actions: &mut DebugUiActions, register: &IoRegisterDisplay, value: u32) {
    for byte in 0..register.width {
        actions.memory_writes.push((
            register.address + u32::from(byte),
            ((value >> (u32::from(byte) * 8)) & 0xFF) as u8,
        ));
    }
}

fn format_address(register: &IoRegisterDisplay) -> String {
    if register.address <= 0xFFFF {
        format!("{:04X}", register.address)
    } else {
        format!("{:08X}", register.address)
    }
}

fn format_value(register: &IoRegisterDisplay) -> String {
    match register.width {
        1 => format!("{:02X}", register.value),
        2 => format!("{:04X}", register.value),
        _ => format!("{:08X}", register.value),
    }
}

fn section_color(ui: &egui::Ui, heading: &str) -> egui::Color32 {
    let colors = debug_colors(ui);
    match heading {
        "Interrupts" => color32(colors.interrupt),
        "Timer" | "PSG" | "APU" => color32(colors.opcode),
        "PPU" | "VDP" | "Video" => color32(colors.selection),
        "Mapper" | "Cartridge" => color32(colors.address),
        _ => ui.visuals().text_color(),
    }
}

fn semantic_line(
    ui: &egui::Ui,
    line: &str,
    font: &egui::FontId,
    base: egui::Color32,
    changed: bool,
) -> egui::text::LayoutJob {
    let colors = debug_colors(ui);
    let mut job = egui::text::LayoutJob::default();
    for (index, token) in line.split_whitespace().enumerate() {
        if index > 0 {
            append(&mut job, font, base, " ");
        }
        if changed {
            append(&mut job, font, color32(colors.changed), token);
            continue;
        }
        let split = token.find([':', '=']).map(|index| index + 1);
        if let Some(value_start) = split {
            append(&mut job, font, base, &token[..value_start]);
            let value = &token[value_start..];
            append(&mut job, font, value_color(ui, value), value);
        } else {
            append(&mut job, font, value_color(ui, token), token);
        }
    }
    job
}

fn value_color(ui: &egui::Ui, value: &str) -> egui::Color32 {
    let colors = debug_colors(ui);
    let normalized = value
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
        .to_ascii_lowercase();
    if matches!(normalized.as_str(), "true" | "on" | "yes" | "enabled") {
        color32(colors.symbol)
    } else if matches!(normalized.as_str(), "false" | "off" | "no" | "disabled") {
        ui.visuals().weak_text_color()
    } else if value.chars().any(|ch| ch.is_ascii_digit()) {
        color32(colors.opcode)
    } else {
        ui.visuals().text_color()
    }
}

fn append(job: &mut egui::text::LayoutJob, font: &egui::FontId, color: egui::Color32, text: &str) {
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::IoBitDisplay;

    #[test]
    fn register_write_uses_little_endian_width() {
        let register = IoRegisterDisplay {
            name: "TEST",
            address: 0x0400_0000,
            value: 0,
            width: 2,
            writable_mask: 0xFFFF,
            bits: vec![IoBitDisplay {
                mask: 1,
                label: "Bit",
            }],
        };
        let mut actions = DebugUiActions::none();
        write_register(&mut actions, &register, 0x1234);
        assert_eq!(
            actions.memory_writes,
            vec![(0x0400_0000, 0x34), (0x0400_0001, 0x12)]
        );
    }
}
