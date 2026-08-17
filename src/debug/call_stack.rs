use crate::debug::common::{color32, debug_colors, debug_mono_font, format_addr};
use crate::debug::{CpuDebugSnapshot, DebugTab, DebugUiActions, DisassemblyTarget};
use crate::symbols::SymbolSession;

#[derive(Clone, Copy)]
struct CallStackLayout {
    symbol: bool,
    return_address: bool,
}

fn call_stack_layout(width: f32) -> CallStackLayout {
    CallStackLayout {
        symbol: width >= 330.0,
        return_address: width >= 470.0,
    }
}

pub(super) fn draw_call_stack_content(
    ui: &mut egui::Ui,
    info: &CpuDebugSnapshot,
    symbols: &SymbolSession,
    actions: &mut DebugUiActions,
) {
    if !info.call_stack_available {
        ui.label("Call stack capture is not supported by this core yet.");
        return;
    }
    ui.weak("Frames captured while debugger history is active");
    if info.call_stack.is_empty() {
        ui.label("No active captured frames.");
        return;
    }

    let mono = debug_mono_font(ui);
    let colors = debug_colors(ui);
    let address_color = color32(colors.address);
    let symbol_color = color32(colors.symbol);
    let source_color = color32(colors.source);
    let interrupt_color = color32(colors.interrupt);
    let layout = call_stack_layout(ui.available_width());

    egui::Grid::new("call_stack_grid")
        .striped(true)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            ui.strong("#");
            ui.strong("Kind");
            ui.strong("Target");
            if layout.symbol {
                ui.strong("Symbol");
            }
            if layout.return_address {
                ui.strong("Return");
            }
            ui.end_row();

            for (depth, frame) in info.call_stack.iter().rev().enumerate() {
                ui.label(
                    egui::RichText::new(depth.to_string())
                        .font(mono.clone())
                        .color(address_color),
                );
                let kind_color = if frame.kind == "INT" {
                    interrupt_color
                } else {
                    source_color
                };
                ui.label(
                    egui::RichText::new(frame.kind)
                        .font(mono.clone())
                        .color(kind_color),
                );

                let target = format_addr(frame.target);
                let target_response = ui.add(
                    egui::Label::new(
                        egui::RichText::new(target)
                            .font(mono.clone())
                            .color(address_color),
                    )
                    .sense(egui::Sense::click()),
                );
                let name = frame
                    .target_rom_offset
                    .and_then(|offset| symbols.symbol_name_at_rom_offset(offset))
                    .or_else(|| symbols.unique_symbol_name_at_cpu_address(frame.target.into()))
                    .unwrap_or("-");
                if layout.symbol {
                    ui.label(
                        egui::RichText::new(name)
                            .font(mono.clone())
                            .color(symbol_color),
                    );
                }

                let return_response = layout.return_address.then(|| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format_addr(frame.return_address))
                                .font(mono.clone())
                                .color(address_color),
                        )
                        .sense(egui::Sense::click()),
                    )
                });
                ui.end_row();

                if target_response
                    .on_hover_text("Open target in Disassembler")
                    .clicked()
                {
                    navigate(actions, frame.target, frame.target_rom_offset);
                }
                if return_response.is_some_and(|response| {
                    response
                        .on_hover_text("Open return address in Disassembler")
                        .clicked()
                }) {
                    navigate(actions, frame.return_address, frame.return_rom_offset);
                }
            }
        });
}

fn navigate(
    actions: &mut DebugUiActions,
    cpu_address: zeff_emu_common::address::Address,
    storage_offset: Option<u64>,
) {
    actions.disasm_target = Some(DisassemblyTarget {
        cpu_address,
        storage_offset,
        thumb: None,
    });
    actions.focus_tab = Some(DebugTab::Disassembler);
}

#[cfg(test)]
mod tests {
    use super::call_stack_layout;

    #[test]
    fn columns_follow_available_width() {
        let narrow = call_stack_layout(300.0);
        assert!(!narrow.symbol);
        assert!(!narrow.return_address);

        let wide = call_stack_layout(600.0);
        assert!(wide.symbol);
        assert!(wide.return_address);
    }
}
