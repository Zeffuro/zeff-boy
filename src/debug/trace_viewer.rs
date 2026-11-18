use std::fmt::Write;

use zeff_emu_common::address::Address;
use zeff_emu_common::debug::{
    InstructionTraceRecord, TraceBoundaryKind, TraceExecMode, TraceFrameKnowledge,
    TraceObservationEnd, TraceObservationStart, TraceWrite, TraceWriteKind, TraceWriteWidth,
};

use super::types::TraceViewerState;
use super::{DebugTab, DebugUiActions, DisassemblyTarget};

mod controls;
mod decode;
#[cfg(test)]
mod target_symbols_tests;
use decode::{decode_entry, trace_cpu_address};

pub(crate) fn draw_trace_content(
    ui: &mut egui::Ui,
    state: &mut TraceViewerState,
    primary_symbols: &crate::symbols::SymbolSession,
    sound_symbols: &crate::symbols::SymbolSession,
    actions: &mut DebugUiActions,
) {
    controls::draw(ui, state, actions);
    let symbols = match state.target {
        crate::ui::TraceTarget::Primary => primary_symbols,
        crate::ui::TraceTarget::Sound => sound_symbols,
    };

    ui.horizontal(|ui| {
        ui.label("Filter");
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .hint_text("symbol, pc:C000, write:C000-C0FF")
                .desired_width(280.0),
        );
    });

    refresh_filter(state, symbols);
    let filtered = !state.filter.trim().is_empty();
    let row_count = if filtered {
        state.filtered_indices.len()
    } else {
        state.entries.len()
    };
    let available_width = ui.available_width();
    let show_source = available_width >= 980.0;
    let show_timing = available_width >= 720.0;
    let show_changes = available_width >= 540.0;
    let mono = super::common::debug_mono_font(ui);
    let colors = super::common::debug_colors(ui);

    ui.separator();
    ui.horizontal(|ui| {
        sized_label(ui, 72.0, "Sequence");
        if show_timing {
            sized_label(ui, 72.0, "Frame");
            sized_label(ui, 160.0, "Cycle / observation start");
        }
        sized_label(ui, 88.0, "PC");
        sized_label(ui, 104.0, "ROM");
        sized_label(ui, 104.0, "Bytes");
        if show_source {
            sized_label(ui, 180.0, "Source");
        }
        ui.weak("Instruction / changes");
    });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(state.auto_scroll)
        .show_rows(ui, 20.0, row_count, |ui, rows| {
            for visible_index in rows {
                let entry_index = if filtered {
                    state.filtered_indices[visible_index]
                } else {
                    visible_index
                };
                let entry = state.entries[entry_index];
                let decoded = decode_entry(&entry);
                let symbol = trace_symbol_for_target(state.target, symbols, &entry);
                let source = (state.target == crate::ui::TraceTarget::Primary)
                    .then_some(entry.physical_rom_offset)
                    .flatten()
                    .and_then(|offset| symbols.source_reference_at_rom_offset(offset));
                let source_label = source.as_ref().map(|source| {
                    std::path::Path::new(&source.display_path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&source.display_path)
                });
                let response = ui
                    .horizontal(|ui| {
                        ui.style_mut().override_font_id = Some(mono.clone());
                        sized_label(ui, 72.0, &entry.sequence.to_string());
                        if show_timing {
                            sized_label(ui, 72.0, &trace_frame_label(&entry));
                            sized_label(ui, 160.0, &format_cycle(&entry));
                        }
                        colored_sized_label(
                            ui,
                            88.0,
                            &format_pc(&entry),
                            super::common::color32(colors.address),
                        );
                        colored_sized_label(
                            ui,
                            104.0,
                            &format_rom(entry.physical_rom_offset),
                            super::common::color32(colors.selection),
                        );
                        sized_label(ui, 104.0, &decoded.bytes);
                        if show_source {
                            sized_label(ui, 180.0, source_label.unwrap_or("--"));
                        }
                        let text = if let Some(symbol) = symbol {
                            format!("{symbol}: {}", decoded.mnemonic)
                        } else {
                            decoded.mnemonic
                        };
                        ui.colored_label(super::common::color32(colors.symbol), text);
                        if show_changes {
                            let changes = format_changes(&entry);
                            if !changes.is_empty() {
                                ui.colored_label(super::common::color32(colors.changed), changes);
                            }
                        }
                    })
                    .response
                    .interact(egui::Sense::click());
                let response = if let Some(source) = &source {
                    response.on_hover_text(format!("{}:{}", source.display_path, source.line))
                } else {
                    response
                };
                if response.clicked()
                    && state.target == crate::ui::TraceTarget::Primary
                    && actions.target == crate::debug::DebugTarget::Primary
                {
                    let target = if entry.mode == TraceExecMode::M68000 {
                        crate::debug::CpuDisassemblyTarget::m68000(Address::from(
                            trace_cpu_address(&entry),
                        ))
                    } else {
                        crate::debug::CpuDisassemblyTarget {
                            cpu_address: Address::from(trace_cpu_address(&entry)),
                            storage_offset: entry.physical_rom_offset,
                            bank: entry.bank,
                            thumb: match entry.mode {
                                TraceExecMode::Thumb => Some(true),
                                TraceExecMode::Arm => Some(false),
                                _ => None,
                            },
                        }
                    };
                    actions.disasm_target = Some(DisassemblyTarget::Cpu(target));
                    actions.focus_tab = Some(DebugTab::Disassembler);
                }
            }
        });
}

fn refresh_filter(state: &mut TraceViewerState, symbols: &crate::symbols::SymbolSession) {
    let sequence = state.entries.back().map(|entry| entry.sequence);
    if state.cached_filter == state.filter && state.cached_sequence == sequence {
        return;
    }
    state.cached_filter.clone_from(&state.filter);
    state.cached_sequence = sequence;
    state.filtered_indices.clear();
    let query = state.filter.trim().to_ascii_lowercase();
    if query.is_empty() {
        return;
    }
    state.filtered_indices.extend(
        state
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                if state.target == crate::ui::TraceTarget::Primary {
                    entry_matches(entry, &query, symbols)
                } else {
                    entry_matches_target(state.target, entry, &query, symbols)
                }
            })
            .map(|(index, _)| index),
    );
}

fn trace_symbol_for_target(
    target: crate::ui::TraceTarget,
    symbols: &crate::symbols::SymbolSession,
    entry: &InstructionTraceRecord,
) -> Option<String> {
    match (target, symbols.target()) {
        (crate::ui::TraceTarget::Primary, crate::debug::DebugTarget::Primary) => {
            trace_symbol_context(symbols, entry)
        }
        (crate::ui::TraceTarget::Sound, crate::debug::DebugTarget::Sound) => symbols
            .unique_symbol_name_at_cpu_address(u64::from(entry.pc))
            .map(str::to_owned),
        _ => None,
    }
}

fn entry_matches(
    entry: &InstructionTraceRecord,
    query: &str,
    symbols: &crate::symbols::SymbolSession,
) -> bool {
    entry_matches_target(crate::ui::TraceTarget::Primary, entry, query, symbols)
}

fn entry_matches_target(
    target: crate::ui::TraceTarget,
    entry: &InstructionTraceRecord,
    query: &str,
    symbols: &crate::symbols::SymbolSession,
) -> bool {
    if let Some(range) = query.strip_prefix("pc:").and_then(parse_range) {
        return range_contains(range, entry.pc);
    }
    if let Some(range) = query.strip_prefix("write:").and_then(parse_range) {
        return entry.writes().iter().any(|write| {
            (0..write.width as u32)
                .any(|offset| range_contains(range, write.address.wrapping_add(offset)))
        });
    }
    trace_symbol_for_target(target, symbols, entry)
        .is_some_and(|symbol| symbol.to_ascii_lowercase().contains(query))
        || format_pc(entry).to_ascii_lowercase().contains(query)
        || entry
            .event
            .is_some_and(|event| event.label().to_ascii_lowercase().contains(query))
}

fn parse_range(value: &str) -> Option<(u32, u32)> {
    let mut parts = value.splitn(2, '-');
    let start = parse_hex(parts.next()?)?;
    let end = parts.next().and_then(parse_hex).unwrap_or(start);
    Some((start.min(end), start.max(end)))
}

fn parse_hex(value: &str) -> Option<u32> {
    u32::from_str_radix(
        value
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches('$'),
        16,
    )
    .ok()
}

fn range_contains(range: (u32, u32), value: u32) -> bool {
    (range.0..=range.1).contains(&value)
}

fn trace_symbol_context(
    symbols: &crate::symbols::SymbolSession,
    entry: &InstructionTraceRecord,
) -> Option<String> {
    entry
        .physical_rom_offset
        .and_then(|offset| symbols.symbol_context_at_rom_offset(offset))
        .or_else(|| {
            if entry.mode == TraceExecMode::HuC6280 {
                return entry
                    .bank
                    .and_then(|bank| u8::try_from(bank).ok())
                    .and_then(|page| {
                        symbols.symbol_name_at_debug_location(
                            crate::symbols::pce::pce_static_location(entry.pc as u16, page),
                        )
                    })
                    .map(str::to_owned);
            }
            symbols
                .unique_symbol_name_at_cpu_address(u64::from(trace_cpu_address(entry)))
                .map(str::to_owned)
        })
}

fn format_changes(entry: &InstructionTraceRecord) -> String {
    let mut text = String::new();
    let suffix = entry.observation.start == TraceObservationStart::Suffix;
    let interrupted = entry.observation.end != TraceObservationEnd::Retired;
    if suffix {
        let _ = write!(
            text,
            "Observation start {}; unknown instruction prefix",
            entry.cycle
        );
    }
    if interrupted {
        if !text.is_empty() {
            text.push_str("; ");
        }
        let _ = write!(
            text,
            "Observation interrupted: {}",
            observation_end_label(entry.observation.end)
        );
    }
    if let Some(boundary) = entry.boundary {
        if suffix || interrupted {
            if !text.is_empty() {
                text.push_str("; ");
            }
            let _ = write!(
                text,
                "Observation span {} CPU clocks; {}",
                boundary.cpu_clocks,
                boundary_label(boundary.kind)
            );
        } else {
            let _ = write!(
                text,
                "{}; {} CPU clocks",
                boundary_label(boundary.kind),
                boundary.cpu_clocks
            );
        }
        if let Some(vector) = boundary.final_exception {
            let _ = write!(text, "; final exception {vector}");
        }
        if boundary.machine_fault {
            text.push_str("; machine fault");
        }
    }
    if let Some(event) = entry.event {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(event.label());
    }
    for delta in entry.register_deltas() {
        if !text.is_empty() {
            text.push(' ');
        }
        let _ = write!(
            text,
            "{}={:X}",
            register_name(entry.mode, delta.register),
            delta.value
        );
    }
    for write in entry.writes() {
        if !text.is_empty() {
            text.push(' ');
        }
        let _ = write!(text, "{}", format_write(write));
    }
    if entry.register_delta_overflow != 0 || entry.write_overflow != 0 {
        let _ = write!(
            text,
            " +{}",
            u32::from(entry.register_delta_overflow) + u32::from(entry.write_overflow)
        );
    }
    if entry.instruction_overflow == u16::MAX {
        let _ = write!(
            text,
            " +at least {} instruction bytes (saturated)",
            entry.instruction_overflow
        );
    } else if entry.instruction_overflow != 0 {
        let _ = write!(text, " +{} instruction bytes", entry.instruction_overflow);
    }
    text
}

fn format_cycle(entry: &InstructionTraceRecord) -> String {
    if entry.observation.start == TraceObservationStart::Suffix {
        format!("Observation start {}", entry.cycle)
    } else {
        entry.cycle.to_string()
    }
}

fn observation_end_label(end: TraceObservationEnd) -> &'static str {
    match end {
        TraceObservationEnd::Retired => "retired",
        TraceObservationEnd::Fault => "fault",
        TraceObservationEnd::Reset => "reset",
        TraceObservationEnd::Disabled => "disabled",
    }
}

fn boundary_label(kind: TraceBoundaryKind) -> &'static str {
    match kind {
        TraceBoundaryKind::Instruction => "Instruction",
        TraceBoundaryKind::Interrupt => "IRQ service",
        TraceBoundaryKind::Trace => "Trace service",
        TraceBoundaryKind::Stopped => "Stopped",
        TraceBoundaryKind::Fault => "Boundary failed",
    }
}

fn format_write(write: &TraceWrite) -> String {
    let prefix = match write.kind {
        TraceWriteKind::Io => "IO",
        TraceWriteKind::Bus => "BUS",
        TraceWriteKind::Memory => "",
    };
    let digits = match write.width {
        TraceWriteWidth::Byte => 2,
        TraceWriteWidth::Halfword => 4,
        TraceWriteWidth::Word => 8,
    };
    let old = write
        .old_value
        .map_or_else(|| "??".into(), |value| format!("{value:0digits$X}"));
    format!(
        "{prefix}[{:X}]={old}>{:0digits$X}",
        write.address, write.new_value
    )
}

fn register_name(mode: TraceExecMode, register: u8) -> &'static str {
    const GB: [&str; 10] = ["A", "F", "B", "C", "D", "E", "H", "L", "SP", "PC"];
    const NES: [&str; 6] = ["A", "X", "Y", "SP", "P", "PC"];
    const HUC6280: [&str; 15] = [
        "A", "X", "Y", "SP", "PC", "P", "MPR0", "MPR1", "MPR2", "MPR3", "MPR4", "MPR5", "MPR6",
        "MPR7", "SPD",
    ];
    const Z80: [&str; 14] = [
        "A", "F", "B", "C", "D", "E", "H", "L", "IX", "IY", "SP", "PC", "I", "R",
    ];
    const V30: [&str; 14] = [
        "AX", "CX", "DX", "BX", "SP", "BP", "SI", "DI", "ES", "CS", "SS", "DS", "IP", "F",
    ];
    const M68000: [&str; 19] = [
        "D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7", "A0", "A1", "A2", "A3", "A4", "A5", "A6",
        "USP", "SSP", "PC", "SR",
    ];
    match mode {
        TraceExecMode::Sm83 => GB.get(register as usize).copied().unwrap_or("?"),
        TraceExecMode::Mos6502 => NES.get(register as usize).copied().unwrap_or("?"),
        TraceExecMode::HuC6280 => HUC6280.get(register as usize).copied().unwrap_or("?"),
        TraceExecMode::Z80 => Z80.get(register as usize).copied().unwrap_or("?"),
        TraceExecMode::V30 => V30.get(register as usize).copied().unwrap_or("?"),
        TraceExecMode::M68000 => M68000.get(register as usize).copied().unwrap_or("?"),
        TraceExecMode::Arm | TraceExecMode::Thumb => match register {
            0 => "R0",
            1 => "R1",
            2 => "R2",
            3 => "R3",
            4 => "R4",
            5 => "R5",
            6 => "R6",
            7 => "R7",
            8 => "R8",
            9 => "R9",
            10 => "R10",
            11 => "R11",
            12 => "R12",
            13 => "SP",
            14 => "LR",
            15 => "PC",
            16 => "CPSR",
            _ => "?",
        },
    }
}

pub(crate) fn trace_frame_label(entry: &InstructionTraceRecord) -> String {
    match entry.observation.frame_knowledge {
        TraceFrameKnowledge::Exact => entry.frame.to_string(),
        TraceFrameKnowledge::Unknown => "Unknown".to_string(),
    }
}

fn format_pc(entry: &InstructionTraceRecord) -> String {
    match entry.mode {
        TraceExecMode::Arm | TraceExecMode::Thumb | TraceExecMode::V30 | TraceExecMode::M68000 => {
            format!("{:08X}", entry.pc)
        }
        _ => format!("{:04X}", entry.pc),
    }
}

fn format_rom(offset: Option<u64>) -> String {
    offset.map_or_else(|| "--".to_string(), |offset| format!("+{offset:06X}"))
}

fn format_capacity(capacity: usize) -> String {
    if capacity >= 1_000 {
        format!("{}k", capacity / 1_000)
    } else {
        capacity.to_string()
    }
}

fn sized_label(ui: &mut egui::Ui, width: f32, text: &str) {
    ui.add_sized([width, 18.0], egui::Label::new(text));
}

fn colored_sized_label(ui: &mut egui::Ui, width: f32, text: &str, color: egui::Color32) {
    ui.add_sized(
        [width, 18.0],
        egui::Label::new(egui::RichText::new(text).color(color)),
    );
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod observation_tests;
