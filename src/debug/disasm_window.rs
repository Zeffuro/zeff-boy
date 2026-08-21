use std::collections::HashSet;
use std::fmt::Write;

use super::common::{color32, debug_colors, debug_mono_font, format_addr};
use crate::debug::{DisassemblyTarget, DisassemblyView};
use zeff_emu_common::address::Address;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DisassemblyLayout {
    byte_limit: usize,
    show_source: bool,
}

fn disassembly_layout(available_width: f32) -> DisassemblyLayout {
    if available_width >= 900.0 {
        DisassemblyLayout {
            byte_limit: 16,
            show_source: true,
        }
    } else if available_width >= 660.0 {
        DisassemblyLayout {
            byte_limit: 8,
            show_source: true,
        }
    } else if available_width >= 440.0 {
        DisassemblyLayout {
            byte_limit: 4,
            show_source: false,
        }
    } else {
        DisassemblyLayout {
            byte_limit: 0,
            show_source: false,
        }
    }
}

fn format_instruction_bytes(bytes: &[u8], limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let truncated = bytes.len() > limit;
    let visible = if truncated {
        limit.saturating_sub(1)
    } else {
        limit
    };
    let mut text = String::with_capacity(limit * 3);
    for (index, byte) in bytes.iter().take(visible).enumerate() {
        if index > 0 {
            text.push(' ');
        }
        let _ = write!(text, "{byte:02X}");
    }
    if truncated {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str("..");
    }
    text
}

pub(crate) struct DisassemblerActions {
    pub(crate) toggle_breakpoints: Vec<Address>,
    pub(crate) toggle_rom_breakpoints: Vec<u64>,
    pub(crate) add_one_shot_breakpoint: Option<Address>,
    pub(crate) step_requested: bool,
    pub(crate) next_frame_requested: bool,
    pub(crate) continue_requested: bool,
    pub(crate) backstep_requested: bool,
    pub(crate) follow_pc_requested: bool,
    pub(crate) back_requested: bool,
    pub(crate) forward_requested: bool,
    pub(crate) disasm_target: Option<DisassemblyTarget>,
}

pub(super) fn draw_disassembler_content(
    ui: &mut egui::Ui,
    view: &DisassemblyView,
    supports_rewind: bool,
) -> DisassemblerActions {
    let mut actions = DisassemblerActions {
        toggle_breakpoints: Vec::new(),
        toggle_rom_breakpoints: Vec::new(),
        add_one_shot_breakpoint: None,
        step_requested: false,
        next_frame_requested: false,
        continue_requested: false,
        backstep_requested: false,
        follow_pc_requested: false,
        back_requested: false,
        forward_requested: false,
        disasm_target: None,
    };
    let mut breakpoints: HashSet<Address> = view.breakpoints.iter().copied().collect();
    let one_shot_breakpoints: HashSet<Address> =
        view.one_shot_breakpoints.iter().copied().collect();
    let mut rom_breakpoints: HashSet<u64> = view.rom_breakpoints.iter().copied().collect();

    ui.horizontal(|ui| {
        if ui.button("Continue (F9)").clicked() {
            actions.continue_requested = true;
        }
        if ui.button("Step (F7)").clicked() {
            actions.step_requested = true;
        }
        if ui.button("Next Frame").clicked() {
            actions.next_frame_requested = true;
        }
        ui.separator();
        if ui
            .add_enabled(supports_rewind, egui::Button::new("Step Back"))
            .on_hover_text("Rewind one snapshot (~4 frames) and pause")
            .clicked()
        {
            actions.backstep_requested = true;
        }
    });

    if view.is_navigation_target {
        ui.horizontal(|ui| {
            ui.label(if view.is_static_target {
                "Viewing ROM target"
            } else {
                "Viewing execution point"
            });
            if ui.small_button("←").on_hover_text("Back").clicked() {
                actions.back_requested = true;
            }
            if ui.small_button("→").on_hover_text("Forward").clicked() {
                actions.forward_requested = true;
            }
            if ui.small_button("Follow PC").clicked() {
                actions.follow_pc_requested = true;
            }
        });
        ui.label(if view.is_static_target {
            "Click toggles a ROM breakpoint. Right-click for more."
        } else {
            "Click toggles a breakpoint. Right-click for more."
        });
    } else {
        ui.label("Click toggles a breakpoint. Right-click for more.");
    }
    if let Some(symbol) = &view.location_symbol {
        ui.horizontal(|ui| {
            ui.weak("Location");
            ui.label(
                egui::RichText::new(symbol)
                    .font(debug_mono_font(ui))
                    .color(color32(debug_colors(ui).symbol)),
            );
        });
    }
    ui.separator();

    let mono = debug_mono_font(ui);
    let layout = disassembly_layout(ui.available_width());
    let normal_color = ui.visuals().text_color();
    let colors = debug_colors(ui);
    let bp_color = color32(colors.breakpoint);

    let fmt_addr = egui::TextFormat {
        font_id: mono.clone(),
        color: color32(colors.address),
        ..Default::default()
    };
    let fmt_normal = egui::TextFormat {
        font_id: mono.clone(),
        color: normal_color,
        ..Default::default()
    };
    let fmt_bp = egui::TextFormat {
        font_id: mono.clone(),
        color: bp_color,
        ..Default::default()
    };
    let fmt_symbol = egui::TextFormat {
        font_id: mono.clone(),
        color: color32(colors.symbol),
        ..Default::default()
    };
    let fmt_bytes = egui::TextFormat {
        font_id: mono.clone(),
        color: color32(colors.opcode),
        ..Default::default()
    };
    let fmt_mnemonic = egui::TextFormat {
        font_id: mono.clone(),
        color: color32(colors.mnemonic),
        ..Default::default()
    };
    let fmt_source = egui::TextFormat {
        font_id: mono.clone(),
        color: color32(colors.source),
        ..Default::default()
    };

    let mut header = egui::text::LayoutJob::default();
    header.append("     ", 0.0, fmt_addr.clone());
    header.append("Addr   ", 0.0, fmt_addr.clone());
    if layout.byte_limit > 0 {
        let width = layout.byte_limit * 3 - 1;
        header.append(&format!("{:<width$} ", "Bytes"), 0.0, fmt_addr.clone());
    }
    header.append("Mnemonic / Symbol", 0.0, fmt_addr.clone());
    ui.label(header);

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut addr_buf = String::with_capacity(8);
        let mut padded = String::with_capacity(12);
        for line in &view.lines {
            let is_pc = line.address == view.pc;
            let has_breakpoint = if view.is_static_target {
                line.storage_offset
                    .is_some_and(|offset| rom_breakpoints.contains(&offset))
            } else {
                breakpoints.contains(&line.address)
            };
            let is_one_shot =
                !view.is_static_target && one_shot_breakpoints.contains(&line.address);

            let scratch = format_instruction_bytes(&line.bytes, layout.byte_limit);

            let mut job = egui::text::LayoutJob::default();

            match (has_breakpoint, is_one_shot, is_pc) {
                (true, true, true) => job.append("1x>  ", 0.0, fmt_bp.clone()),
                (true, true, false) => job.append("1x   ", 0.0, fmt_bp.clone()),
                (true, false, true) => job.append("BP>  ", 0.0, fmt_bp.clone()),
                (true, false, false) => job.append("BP   ", 0.0, fmt_bp.clone()),
                (false, _, true) => job.append("  >  ", 0.0, fmt_addr.clone()),
                (false, _, false) => job.append("     ", 0.0, fmt_normal.clone()),
            }

            addr_buf.clear();
            let _ = write!(addr_buf, "{}: ", format_addr(line.address));
            job.append(&addr_buf, 0.0, fmt_addr.clone());

            let mut row_bytes = fmt_bytes.clone();
            let mut row_mnemonic = fmt_mnemonic.clone();
            if is_pc {
                row_bytes.background = color32(colors.pc);
                row_mnemonic.background = color32(colors.pc);
            }
            if layout.byte_limit > 0 {
                let width = layout.byte_limit * 3 - 1;
                padded.clear();
                let _ = write!(padded, "{:<width$} ", scratch);
                job.append(&padded, 0.0, row_bytes);
            }
            job.append(&line.mnemonic, 0.0, row_mnemonic);
            if let Some(symbol) = &line.symbol {
                job.append("  ", 0.0, fmt_normal.clone());
                job.append(symbol, 0.0, fmt_symbol.clone());
            }
            if let Some(symbol) = &line.control_target_symbol {
                job.append("  -> ", 0.0, fmt_normal.clone());
                job.append(symbol, 0.0, fmt_symbol.clone());
            }
            if layout.show_source
                && let Some(source) = &line.source
            {
                job.append("  src ", 0.0, fmt_normal.clone());
                job.append(source, 0.0, fmt_source.clone());
            }

            let mut label = ui.add(egui::Label::new(job).sense(egui::Sense::click()));
            let mut hover = Vec::new();
            if line.control_target_storage.is_some() {
                hover.push("Double-click to follow target".to_owned());
            }
            if !layout.show_source
                && let Some(source) = &line.source
            {
                hover.push(format!("Source: {source}"));
            }
            if line.bytes.len() > layout.byte_limit {
                hover.push(format!(
                    "Bytes: {}",
                    format_instruction_bytes(&line.bytes, 16)
                ));
            }
            if !hover.is_empty() {
                label = label.on_hover_text(hover.join("\n"));
            }
            label.context_menu(|ui| {
                let toggle_label = if has_breakpoint {
                    "Remove breakpoint"
                } else {
                    "Add breakpoint"
                };
                if ui.button(toggle_label).clicked() {
                    if view.is_static_target {
                        if let Some(offset) = line.storage_offset {
                            actions.toggle_rom_breakpoints.push(offset);
                        }
                    } else {
                        actions.toggle_breakpoints.push(line.address);
                    }
                    ui.close();
                }
                if !view.is_static_target {
                    if ui.button("Break once").clicked() {
                        actions.add_one_shot_breakpoint = Some(line.address);
                        ui.close();
                    }
                    if ui.button("Run to cursor").clicked() {
                        actions.add_one_shot_breakpoint = Some(line.address);
                        actions.continue_requested = true;
                        ui.close();
                    }
                }
                if let (Some(cpu_address), Some(storage_offset)) =
                    (line.control_target, line.control_target_storage)
                    && ui.button("Follow target").clicked()
                {
                    actions.disasm_target = Some(DisassemblyTarget {
                        cpu_address,
                        storage_offset: Some(storage_offset),
                        thumb: None,
                    });
                    ui.close();
                }
                ui.separator();
                if ui.button("Copy address").clicked() {
                    ui.ctx().copy_text(format_addr(line.address));
                    ui.close();
                }
                if let Some(symbol) = &line.symbol
                    && ui.button("Copy symbol").clicked()
                {
                    ui.ctx().copy_text(symbol.clone());
                    ui.close();
                }
            });
            if label.double_clicked()
                && let (Some(cpu_address), Some(storage_offset)) =
                    (line.control_target, line.control_target_storage)
            {
                actions.disasm_target = Some(DisassemblyTarget {
                    cpu_address,
                    storage_offset: Some(storage_offset),
                    thumb: None,
                });
            } else if label.clicked() {
                if view.is_static_target {
                    if let Some(offset) = line.storage_offset {
                        actions.toggle_rom_breakpoints.push(offset);
                        if has_breakpoint {
                            rom_breakpoints.remove(&offset);
                        } else {
                            rom_breakpoints.insert(offset);
                        }
                    }
                } else {
                    actions.toggle_breakpoints.push(line.address);
                    if has_breakpoint {
                        breakpoints.remove(&line.address);
                    } else {
                        breakpoints.insert(line.address);
                    }
                }
            }
        }
    });

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disassembly_layout_reduces_optional_columns() {
        assert_eq!(disassembly_layout(1000.0).byte_limit, 16);
        assert_eq!(disassembly_layout(700.0).byte_limit, 8);
        assert_eq!(disassembly_layout(500.0).byte_limit, 4);
        assert_eq!(disassembly_layout(360.0).byte_limit, 0);
        assert!(!disassembly_layout(500.0).show_source);
    }

    #[test]
    fn instruction_bytes_truncate_to_column() {
        assert_eq!(format_instruction_bytes(&[0x01, 0x02], 4), "01 02");
        assert_eq!(
            format_instruction_bytes(&[0x01, 0x02, 0x03, 0x04, 0x05], 4),
            "01 02 03 .."
        );
        assert!(format_instruction_bytes(&[0x01], 0).is_empty());
    }
}
