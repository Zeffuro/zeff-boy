use std::fmt::Write;

use zeff_emu_common::address::Address;
use zeff_emu_common::debug::{
    InstructionTraceRecord, TraceExecMode, TraceWrite, TraceWriteKind, TraceWriteWidth,
};

use super::types::TraceViewerState;
use super::{DebugTab, DebugUiActions, DisassemblyTarget};

pub(crate) fn draw_trace_content(
    ui: &mut egui::Ui,
    state: &mut TraceViewerState,
    symbols: &crate::symbols::SymbolSession,
    actions: &mut DebugUiActions,
) {
    ui.horizontal_wrapped(|ui| {
        let label = if state.enabled { "Stop" } else { "Record" };
        if ui.button(label).clicked() {
            actions.trace_enabled = Some(!state.enabled);
        }
        if ui.button("Clear").clicked() {
            actions.trace_clear = true;
            state.clear();
        }
        egui::ComboBox::from_id_salt("trace_capacity")
            .selected_text(format_capacity(state.capacity))
            .show_ui(ui, |ui| {
                for capacity in [1_000, 10_000, 50_000, 100_000] {
                    if ui
                        .selectable_label(state.capacity == capacity, format_capacity(capacity))
                        .clicked()
                    {
                        actions.trace_capacity = Some(capacity);
                    }
                }
            });
        ui.checkbox(&mut state.auto_scroll, "Follow");
        ui.label(format!("{} / {} retained", state.retained, state.capacity));
        if state.missed != 0 {
            ui.colored_label(
                super::common::color32(super::common::debug_colors(ui).breakpoint),
                format!("{} missed", state.missed),
            );
        }
    });

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
            sized_label(ui, 88.0, "Cycle");
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
                let symbol = trace_symbol_context(symbols, &entry);
                let source = entry
                    .physical_rom_offset
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
                            sized_label(ui, 72.0, &entry.frame.to_string());
                            sized_label(ui, 88.0, &entry.cycle.to_string());
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
                if response.clicked() {
                    actions.disasm_target = Some(DisassemblyTarget {
                        cpu_address: Address::from(entry.pc),
                        storage_offset: entry.physical_rom_offset,
                        thumb: match entry.mode {
                            TraceExecMode::Thumb => Some(true),
                            TraceExecMode::Arm => Some(false),
                            _ => None,
                        },
                    });
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
            .filter(|(_, entry)| entry_matches(entry, &query, symbols))
            .map(|(index, _)| index),
    );
}

fn entry_matches(
    entry: &InstructionTraceRecord,
    query: &str,
    symbols: &crate::symbols::SymbolSession,
) -> bool {
    if let Some(range) = query.strip_prefix("pc:").and_then(parse_range) {
        return range_contains(range, entry.pc);
    }
    if let Some(range) = query.strip_prefix("write:").and_then(parse_range) {
        return entry
            .writes()
            .iter()
            .any(|write| range_contains(range, write.address));
    }
    trace_symbol_context(symbols, entry)
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
            symbols
                .unique_symbol_name_at_cpu_address(u64::from(entry.pc))
                .map(str::to_owned)
        })
}

struct DecodedTrace {
    bytes: String,
    mnemonic: String,
}

fn decode_entry(entry: &InstructionTraceRecord) -> DecodedTrace {
    if entry.instruction_len == 0 {
        return DecodedTrace {
            bytes: "--".to_string(),
            mnemonic: entry
                .event
                .map_or_else(|| "Idle".to_string(), |event| event.label().to_string()),
        };
    }
    let bytes = &entry.instruction[..usize::from(entry.instruction_len)];
    let line = match entry.mode {
        TraceExecMode::Sm83 => super::disassemble_around(
            |address| trace_read16(bytes, entry.pc as u16, address),
            entry.pc as u16,
            0,
            1,
        ),
        TraceExecMode::Mos6502 => super::nes_disassemble_around(
            |address| trace_read16(bytes, entry.pc as u16, address),
            entry.pc as u16,
            0,
            1,
        ),
        TraceExecMode::Z80 => super::z80_disassemble_around(
            |address| trace_read16(bytes, entry.pc as u16, address),
            entry.pc as u16,
            0,
            1,
        ),
        TraceExecMode::Arm | TraceExecMode::Thumb => super::gba_disassemble_around(
            |address| trace_read32(bytes, entry.pc, address),
            entry.pc,
            entry.mode == TraceExecMode::Thumb,
            0,
            1,
        ),
        TraceExecMode::V30 => super::v30_disassemble_around(
            |address| trace_read32(bytes, entry.pc, address),
            entry.pc,
            0,
            1,
        ),
    }
    .into_iter()
    .next();
    if let Some(line) = line {
        DecodedTrace {
            bytes: line
                .bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" "),
            mnemonic: line.mnemonic.to_string(),
        }
    } else {
        DecodedTrace {
            bytes: format!("{:02X}", bytes[0]),
            mnemonic: format!("DB ${:02X}", bytes[0]),
        }
    }
}

fn trace_read16(bytes: &[u8], pc: u16, address: u16) -> u8 {
    bytes
        .get(usize::from(address.wrapping_sub(pc)))
        .copied()
        .unwrap_or(0)
}

fn trace_read32(bytes: &[u8], pc: u32, address: u32) -> u8 {
    bytes
        .get(address.wrapping_sub(pc) as usize)
        .copied()
        .unwrap_or(0)
}

fn format_changes(entry: &InstructionTraceRecord) -> String {
    let mut text = String::new();
    if let Some(event) = entry.event {
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
    text
}

fn format_write(write: &TraceWrite) -> String {
    let prefix = if write.kind == TraceWriteKind::Io {
        "IO"
    } else {
        ""
    };
    let digits = match write.width {
        TraceWriteWidth::Byte => 2,
        TraceWriteWidth::Halfword => 4,
        TraceWriteWidth::Word => 8,
    };
    format!(
        "{prefix}[{:X}]={:0digits$X}>{:0digits$X}",
        write.address, write.old_value, write.new_value
    )
}

fn register_name(mode: TraceExecMode, register: u8) -> &'static str {
    const GB: [&str; 10] = ["A", "F", "B", "C", "D", "E", "H", "L", "SP", "PC"];
    const NES: [&str; 6] = ["A", "X", "Y", "SP", "P", "PC"];
    const Z80: [&str; 14] = [
        "A", "F", "B", "C", "D", "E", "H", "L", "IX", "IY", "SP", "PC", "I", "R",
    ];
    const V30: [&str; 14] = [
        "AX", "CX", "DX", "BX", "SP", "BP", "SI", "DI", "ES", "CS", "SS", "DS", "IP", "F",
    ];
    match mode {
        TraceExecMode::Sm83 => GB.get(register as usize).copied().unwrap_or("?"),
        TraceExecMode::Mos6502 => NES.get(register as usize).copied().unwrap_or("?"),
        TraceExecMode::Z80 => Z80.get(register as usize).copied().unwrap_or("?"),
        TraceExecMode::V30 => V30.get(register as usize).copied().unwrap_or("?"),
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

fn format_pc(entry: &InstructionTraceRecord) -> String {
    match entry.mode {
        TraceExecMode::Arm | TraceExecMode::Thumb | TraceExecMode::V30 => {
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
