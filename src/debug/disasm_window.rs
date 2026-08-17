use std::collections::HashSet;
use std::fmt::Write;

use super::common::{color32, debug_colors, debug_mono_font, format_addr};
use crate::debug::{DisassemblyTarget, DisassemblyView};
use zeff_emu_common::address::Address;

pub(crate) struct DisassemblerActions {
    pub(crate) toggle_breakpoints: Vec<Address>,
    pub(crate) toggle_rom_breakpoints: Vec<u64>,
    pub(crate) step_requested: bool,
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
) -> DisassemblerActions {
    let mut actions = DisassemblerActions {
        toggle_breakpoints: Vec::new(),
        toggle_rom_breakpoints: Vec::new(),
        step_requested: false,
        continue_requested: false,
        backstep_requested: false,
        follow_pc_requested: false,
        back_requested: false,
        forward_requested: false,
        disasm_target: None,
    };
    let mut breakpoints: HashSet<Address> = view.breakpoints.iter().copied().collect();
    let mut rom_breakpoints: HashSet<u64> = view.rom_breakpoints.iter().copied().collect();

    ui.horizontal(|ui| {
        if ui.button("▶ Continue (F9)").clicked() {
            actions.continue_requested = true;
        }
        if ui.button("⏭ Step (F7)").clicked() {
            actions.step_requested = true;
        }
        ui.separator();
        if ui
            .button("⏮ Step Back")
            .on_hover_text("Rewind one snapshot (~4 frames) and pause")
            .clicked()
        {
            actions.backstep_requested = true;
        }
    });

    if view.is_navigation_target {
        ui.horizontal(|ui| {
            ui.label("Viewing ROM target");
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
        ui.label("Click a line to toggle a ROM breakpoint.");
    } else {
        ui.label("Click a line to toggle breakpoint.");
    }
    ui.separator();

    let mono = debug_mono_font(ui);
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

    let mut header = egui::text::LayoutJob::default();
    header.append("     ", 0.0, fmt_addr.clone());
    header.append("Addr   ", 0.0, fmt_addr.clone());
    header.append("Bytes       ", 0.0, fmt_addr.clone());
    header.append("Mnemonic / Symbol", 0.0, fmt_addr.clone());
    ui.label(header);

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut scratch = String::with_capacity(16);
        let mut addr_buf = String::with_capacity(8);
        let mut padded = String::with_capacity(12);
        for line in &view.lines {
            let is_pc = line.address == view.pc;
            let has_breakpoint = if view.is_navigation_target {
                line.storage_offset
                    .is_some_and(|offset| rom_breakpoints.contains(&offset))
            } else {
                breakpoints.contains(&line.address)
            };

            scratch.clear();
            for (i, b) in line.bytes.iter().enumerate() {
                if i > 0 {
                    scratch.push(' ');
                }
                let _ = write!(scratch, "{:02X}", b);
            }

            let mut job = egui::text::LayoutJob::default();

            if has_breakpoint {
                job.append("BP   ", 0.0, fmt_bp.clone());
            } else {
                job.append("     ", 0.0, fmt_normal.clone());
            }

            addr_buf.clear();
            let _ = write!(addr_buf, "{}: ", format_addr(line.address));
            job.append(&addr_buf, 0.0, fmt_addr.clone());

            let mut fmt_code = fmt_normal.clone();
            if is_pc {
                fmt_code.background = color32(colors.pc);
            }
            padded.clear();
            let _ = write!(padded, "{:<11} ", scratch);
            job.append(&padded, 0.0, fmt_code.clone());
            job.append(&line.mnemonic, 0.0, fmt_code);
            if let Some(symbol) = &line.symbol {
                job.append("  ", 0.0, fmt_normal.clone());
                job.append(symbol, 0.0, fmt_symbol.clone());
            }
            if let Some(symbol) = &line.control_target_symbol {
                job.append("  â†’ ", 0.0, fmt_normal.clone());
                job.append(symbol, 0.0, fmt_symbol.clone());
            }

            let mut label = ui.add(egui::Label::new(job).sense(egui::Sense::click()));
            if line.control_target_storage.is_some() {
                label = label.on_hover_text("Double-click to follow target");
            }
            if label.double_clicked()
                && let (Some(cpu_address), Some(storage_offset)) =
                    (line.control_target, line.control_target_storage)
            {
                actions.disasm_target = Some(DisassemblyTarget {
                    cpu_address,
                    storage_offset,
                });
            } else if label.clicked() {
                if view.is_navigation_target {
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
