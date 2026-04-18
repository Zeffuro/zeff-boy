use std::collections::HashSet;
use std::fmt::Write;

use super::common::{COLOR_ADDR, COLOR_PC_HIGHLIGHT_BG, DEBUG_MONO_FONT_SIZE};
use crate::debug::DisassemblyView;

pub(crate) struct DisassemblerActions {
    pub(crate) toggle_breakpoints: Vec<u16>,
    pub(crate) step_requested: bool,
    pub(crate) continue_requested: bool,
    pub(crate) backstep_requested: bool,
}

pub(super) fn draw_disassembler_content(
    ui: &mut egui::Ui,
    view: &DisassemblyView,
) -> DisassemblerActions {
    let mut actions = DisassemblerActions {
        toggle_breakpoints: Vec::new(),
        step_requested: false,
        continue_requested: false,
        backstep_requested: false,
    };
    let mut breakpoints: HashSet<u16> = view.breakpoints.iter().copied().collect();

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

    ui.label("Click a line to toggle breakpoint.");
    ui.separator();

    let mono = egui::FontId::new(DEBUG_MONO_FONT_SIZE, egui::FontFamily::Monospace);
    let normal_color = ui.visuals().text_color();
    let bp_color = egui::Color32::RED;

    let fmt_addr = egui::TextFormat {
        font_id: mono.clone(),
        color: COLOR_ADDR,
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

    let mut header = egui::text::LayoutJob::default();
    header.append("     ", 0.0, fmt_addr.clone());
    header.append("Addr   ", 0.0, fmt_addr.clone());
    header.append("Bytes       ", 0.0, fmt_addr.clone());
    header.append("Mnemonic", 0.0, fmt_addr.clone());
    ui.label(header);

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut scratch = String::with_capacity(16);
        let mut addr_buf = String::with_capacity(8);
        let mut padded = String::with_capacity(12);
        for line in &view.lines {
            let is_pc = line.address == view.pc;
            let has_breakpoint = breakpoints.contains(&line.address);

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
            let _ = write!(addr_buf, "{:04X}: ", line.address);
            job.append(&addr_buf, 0.0, fmt_addr.clone());

            let mut fmt_code = fmt_normal.clone();
            if is_pc {
                fmt_code.background = COLOR_PC_HIGHLIGHT_BG;
            }
            padded.clear();
            let _ = write!(padded, "{:<11} ", scratch);
            job.append(&padded, 0.0, fmt_code.clone());
            job.append(&line.mnemonic, 0.0, fmt_code);

            let label = ui.add(egui::Label::new(job).sense(egui::Sense::click()));
            if label.clicked() {
                actions.toggle_breakpoints.push(line.address);
                if has_breakpoint {
                    breakpoints.remove(&line.address);
                } else {
                    breakpoints.insert(line.address);
                }
            }
        }
    });

    actions
}
