use std::fmt::Write;

use crate::debug::common::{color32, debug_colors, debug_mono_font, format_addr};
use crate::debug::{CpuDebugSnapshot, DebugTab, DebugUiActions, DisassemblyTarget};
use crate::symbols::SymbolSession;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HistoryLayout {
    show_bytes: bool,
    show_source: bool,
    show_detail: bool,
}

fn history_layout(width: f32) -> HistoryLayout {
    HistoryLayout {
        show_bytes: width >= 420.0,
        show_source: width >= 780.0,
        show_detail: width >= 650.0,
    }
}

pub(super) fn draw_execution_history_content(
    ui: &mut egui::Ui,
    info: &CpuDebugSnapshot,
    symbols: &SymbolSession,
    actions: &mut DebugUiActions,
) {
    if info.recent_opcodes.is_empty() {
        ui.label("No execution history available.");
        return;
    }

    let layout = history_layout(ui.available_width());
    let mono = debug_mono_font(ui);
    let colors = debug_colors(ui);
    let address = color32(colors.address);
    let opcode = color32(colors.opcode);
    let symbol = color32(colors.symbol);
    let normal = ui.visuals().text_color();
    ui.horizontal_wrapped(|ui| {
        ui.weak("Last completed");
        ui.label(
            egui::RichText::new(format_addr(info.recent_opcodes[0].address))
                .font(mono.clone())
                .color(address),
        );
        ui.weak("Current PC");
        ui.label(
            egui::RichText::new(format_addr(info.pc))
                .font(mono.clone())
                .color(color32(colors.selection))
                .strong(),
        );
    })
    .response
    .on_hover_text("Branches, calls, returns and interrupts can move the current PC.");

    let available = ui.available_width();
    let symbol_width = if layout.show_source {
        180.0
    } else if layout.show_detail {
        220.0
    } else {
        (available - if layout.show_bytes { 220.0 } else { 105.0 }).max(90.0)
    };
    egui::Grid::new("execution_history_grid")
        .striped(true)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            ui.weak("");
            ui.weak("Address");
            if layout.show_bytes {
                ui.weak("Bytes");
            }
            ui.weak("Runs");
            ui.weak("Symbol");
            if layout.show_source {
                ui.weak("Source");
            }
            if layout.show_detail {
                ui.weak("Detail");
            }
            ui.end_row();

            for (index, entry) in info.recent_opcodes.iter().enumerate() {
                ui.label(
                    egui::RichText::new(if index == 0 { ">" } else { "" })
                        .font(mono.clone())
                        .color(color32(colors.selection))
                        .strong(),
                );
                let mut response = ui.add(
                    egui::Label::new(
                        egui::RichText::new(format_addr(entry.address))
                            .font(mono.clone())
                            .color(address),
                    )
                    .sense(egui::Sense::click()),
                );

                let mut bytes = String::new();
                for (byte_index, byte) in entry.bytes.iter().enumerate() {
                    if byte_index > 0 {
                        bytes.push(' ');
                    }
                    let _ = write!(bytes, "{byte:02X}");
                }
                if layout.show_bytes {
                    response = response.union(
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&bytes).font(mono.clone()).color(opcode),
                            )
                            .sense(egui::Sense::click()),
                        ),
                    );
                }

                let repeats = if entry.repeat_count > 1 {
                    format!("x{}", entry.repeat_count)
                } else {
                    "-".to_owned()
                };
                response = response.union(
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(repeats)
                                .font(mono.clone())
                                .color(normal),
                        )
                        .sense(egui::Sense::click()),
                    ),
                );

                let name = entry
                    .storage_offset
                    .and_then(|offset| symbols.symbol_name_at_rom_offset(offset))
                    .or_else(|| symbols.unique_symbol_name_at_cpu_address(entry.address.into()))
                    .unwrap_or("-");
                let name_color = if name == "-" { normal } else { symbol };
                response = response.union(
                    ui.add_sized(
                        [symbol_width, mono.size + 4.0],
                        egui::Label::new(
                            egui::RichText::new(name)
                                .font(mono.clone())
                                .color(name_color),
                        )
                        .truncate()
                        .sense(egui::Sense::click()),
                    ),
                );

                if layout.show_source {
                    let source = entry
                        .storage_offset
                        .and_then(|offset| symbols.source_reference_at_rom_offset(offset))
                        .map(|source| {
                            let file = source
                                .display_path
                                .rsplit(['/', '\\'])
                                .next()
                                .unwrap_or(source.display_path.as_str());
                            format!("{file}:{}", source.line)
                        })
                        .unwrap_or_else(|| "-".to_owned());
                    response = response.union(
                        ui.add_sized(
                            [160.0, mono.size + 4.0],
                            egui::Label::new(
                                egui::RichText::new(source).color(color32(colors.source)),
                            )
                            .truncate()
                            .sense(egui::Sense::click()),
                        ),
                    );
                }
                if layout.show_detail {
                    response = response.union(
                        ui.add_sized(
                            [120.0, mono.size + 4.0],
                            egui::Label::new(
                                egui::RichText::new(entry.detail.as_deref().unwrap_or("-"))
                                    .font(mono.clone())
                                    .color(normal),
                            )
                            .truncate()
                            .sense(egui::Sense::click()),
                        ),
                    );
                }
                ui.end_row();

                let mut hover = Vec::new();
                if !layout.show_bytes {
                    hover.push(format!("Bytes: {bytes}"));
                }
                if !layout.show_detail
                    && let Some(detail) = &entry.detail
                {
                    hover.push(detail.clone());
                }
                hover.push("Click to open in Disassembler".to_owned());
                let clicked = response.clicked();
                response.on_hover_text(hover.join("\n"));
                if clicked {
                    actions.disasm_target = Some(DisassemblyTarget {
                        cpu_address: entry.address,
                        storage_offset: entry.storage_offset,
                        bank: None,
                        thumb: entry.thumb,
                    });
                    actions.focus_tab = Some(DebugTab::Disassembler);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_columns_follow_available_width() {
        assert!(!history_layout(360.0).show_bytes);
        assert!(history_layout(500.0).show_bytes);
        assert!(!history_layout(500.0).show_detail);
        assert!(history_layout(700.0).show_detail);
        assert!(!history_layout(700.0).show_source);
        assert!(history_layout(800.0).show_source);
    }
}
