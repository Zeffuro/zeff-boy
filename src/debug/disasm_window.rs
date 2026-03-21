use std::collections::HashSet;

use crate::debug::DisassemblyView;

pub(crate) fn draw_disassembler_window(
    ctx: &egui::Context,
    view: &DisassemblyView,
    open: &mut bool,
) -> Vec<u16> {
    let mut toggles = Vec::new();
    let breakpoints: HashSet<u16> = view.breakpoints.iter().copied().collect();

    egui::Window::new("Disassembler")
        .open(open)
        .default_width(640.0)
        .show(ctx, |ui| {
            ui.label("Click a line to toggle breakpoint.");
            ui.separator();

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
                    let row = format!("{:04X}: {:<11} {}", line.address, bytes, line.mnemonic);

                    ui.horizontal(|ui| {
                        if has_breakpoint {
                            ui.colored_label(egui::Color32::RED, "BP");
                        } else {
                            ui.label("  ");
                        }

                        let text = if is_pc {
                            egui::RichText::new(row)
                                .background_color(egui::Color32::from_rgb(45, 65, 45))
                        } else {
                            egui::RichText::new(row)
                        };

                        if ui
                            .add(
                                egui::Button::new(text)
                                    .frame(false)
                                    .sense(egui::Sense::click()),
                            )
                            .clicked()
                        {
                            toggles.push(line.address);
                        }
                    });
                }
            });
        });

    toggles
}
