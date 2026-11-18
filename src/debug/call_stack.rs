use crate::debug::common::{color32, debug_colors, debug_mono_font, format_addr};
use crate::debug::{CpuDebugSnapshot, DebugTab, DebugUiActions, DisassemblyTarget};
use crate::symbols::{ExecMode, SymbolSession};

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
                    navigate(
                        actions,
                        symbols.exec_mode(),
                        frame.target,
                        frame.target_rom_offset,
                    );
                }
                if return_response.is_some_and(|response| {
                    response
                        .on_hover_text("Open return address in Disassembler")
                        .clicked()
                }) {
                    navigate(
                        actions,
                        symbols.exec_mode(),
                        frame.return_address,
                        frame.return_rom_offset,
                    );
                }
            }
        });
}

fn navigate(
    actions: &mut DebugUiActions,
    mode: ExecMode,
    cpu_address: zeff_emu_common::address::Address,
    storage_offset: Option<u64>,
) {
    let (cpu_address, storage_offset) = if mode == ExecMode::M68000 {
        (cpu_address & 0xffffff, None)
    } else {
        (cpu_address, storage_offset)
    };
    actions.disasm_target = Some(DisassemblyTarget::Cpu(crate::debug::CpuDisassemblyTarget {
        cpu_address,
        storage_offset,
        bank: None,
        thumb: None,
    }));
    actions.focus_tab = Some(DebugTab::Disassembler);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_follow_available_width() {
        let narrow = call_stack_layout(300.0);
        assert!(!narrow.symbol);
        assert!(!narrow.return_address);

        let wide = call_stack_layout(600.0);
        assert!(wide.symbol);
        assert!(wide.return_address);
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn md_observed_alias_navigation_preserves_raw_frame_and_selects_bus_row() {
        let mut rom = vec![0; 0x800];
        rom[..4].copy_from_slice(&0xfffffc_u32.to_be_bytes());
        rom[4..8].copy_from_slice(&0x80000200_u32.to_be_bytes());
        rom[0x200..0x20a].copy_from_slice(&[0x4e, 0xb9, 0, 0, 4, 0, 0x4e, 0x71, 0x4e, 0x71]);
        rom[0x400..0x404].copy_from_slice(&[0x4e, 0x75, 0x4e, 0x71]);
        let mut machine = zeff_md_core::Emulator::new(rom, Default::default()).unwrap();
        machine.set_call_stack_recording(true);
        machine.step_instruction().unwrap();
        let frame = *machine.call_stack().next().unwrap();
        assert_eq!(frame.return_pc, 0x80000206);
        let before = machine.encode_debugger_checkpoint().unwrap();
        let mut actions = DebugUiActions::none();
        navigate(
            &mut actions,
            ExecMode::M68000,
            frame.return_pc,
            Some(0x80000206),
        );
        let navigation_target = actions.disasm_target.unwrap();
        let target = navigation_target.as_cpu().unwrap();
        assert_eq!(target.cpu_address, 0x206);
        assert!(target.storage_offset.is_none() && target.bank.is_none() && target.thumb.is_none());
        let request = crate::emu_thread::SnapshotRequest {
            debug_target: crate::debug::DebugTarget::Primary,
            debug_revision: 0,
            want_debug_info: true,
            want_perf_info: false,
            any_viewer_open: false,
            any_vram_viewer_open: false,
            show_oam_viewer: false,
            show_apu_viewer: false,
            show_disassembler: true,
            show_rom_info: false,
            show_memory_viewer: false,
            memory_view_start: 0,
            show_rom_viewer: false,
            show_instruction_trace: false,
            trace_cursor: Default::default(),
            rom_view_start: 0,
            last_disasm_pc: None,
            last_disasm_mapping: None,
            disasm_target: Some(navigation_target),
            memory_search: None,
            rom_search: None,
            render: crate::emu_thread::RenderSettings {
                color_correction: Default::default(),
                color_correction_matrix: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                dmg_palette_preset: Default::default(),
                nes_palette_mode: Default::default(),
                nes_custom_palette: None,
                pce_overscan_mode: Default::default(),
                pce_palette_mode: Default::default(),
                sgb_border_enabled: false,
            },
        };
        let view = crate::ui::collect_md_snapshot(&machine, &request, None);
        let disassembly = view.disassembly_view.unwrap();
        assert!(disassembly.is_navigation_target);
        assert_eq!(disassembly.lines[0].address, 0x206);
        assert_eq!(disassembly.lines[0].mnemonic.as_str(), "NOP");
        assert_ne!(disassembly.lines[0].address, machine.registers().pc);
        assert_eq!(
            view.cpu_debug.unwrap().call_stack[0].return_address,
            0x80000206
        );
        assert_eq!(machine.encode_debugger_checkpoint().unwrap(), before);
        for mode in [ExecMode::Sm83, ExecMode::Arm, ExecMode::Thumb] {
            navigate(&mut actions, mode, 0x12345678, Some(0x480200));
            let target = actions.disasm_target.unwrap();
            let target = target.as_cpu().unwrap();
            assert_eq!(target.cpu_address, 0x12345678);
            assert_eq!(target.storage_offset, Some(0x480200));
        }
    }
}
