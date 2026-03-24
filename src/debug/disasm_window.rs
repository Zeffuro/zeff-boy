use std::collections::HashSet;

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
        if ui.button("▶ Continue (F5)").clicked() {
            actions.continue_requested = true;
        }
        if ui.button("⏭ Step (F10)").clicked() {
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

    let mono = egui::FontId::new(13.0, egui::FontFamily::Monospace);
    let normal_color = ui.visuals().text_color();
    let addr_color = egui::Color32::from_rgb(140, 140, 170);
    let bp_color = egui::Color32::RED;
    let pc_bg = egui::Color32::from_rgb(45, 65, 45);

    let fmt_addr = egui::TextFormat {
        font_id: mono.clone(),
        color: addr_color,
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
        for line in &view.lines {
            let is_pc = line.address == view.pc;
            let has_breakpoint = breakpoints.contains(&line.address);
            let bytes = line
                .bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");

            let mut job = egui::text::LayoutJob::default();

            if has_breakpoint {
                job.append("BP   ", 0.0, fmt_bp.clone());
            } else {
                job.append("     ", 0.0, fmt_normal.clone());
            }

            job.append(&format!("{:04X}: ", line.address), 0.0, fmt_addr.clone());

            let mut fmt_code = fmt_normal.clone();
            if is_pc {
                fmt_code.background = pc_bg;
            }
            job.append(&format!("{:<11} ", bytes), 0.0, fmt_code.clone());
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
