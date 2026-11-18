use crate::debug::BreakpointState;
use crate::debug::common::{
    COLOR_CONTINUE_BUTTON, WatchType, color32, debug_colors, format_addr, parse_hex_u32,
    parse_hex_u64,
};
use crate::debug::types::CpuDebugSnapshot;
use crate::debug::ui::DebugUiActions;
use crate::symbols::{ExecMode, SymbolRecord, SymbolSession};
use zeff_emu_common::address::Address;
use zeff_emu_common::system::CoreFamily;

pub(super) struct BreakpointContext<'a> {
    pub(super) core_family: Option<CoreFamily>,
    pub(super) memory_watchpoints: bool,
    pub(super) rom_breakpoints: bool,
    pub(super) symbols: &'a SymbolSession,
}

pub(super) fn draw_breakpoints_content(
    ui: &mut egui::Ui,
    info: &CpuDebugSnapshot,
    context: BreakpointContext<'_>,
    state: &mut BreakpointState,
    actions: &mut DebugUiActions,
) {
    let BreakpointContext {
        core_family,
        memory_watchpoints,
        rom_breakpoints,
        symbols,
    } = context;
    let m68000 = core_family == Some(CoreFamily::MegaDrive);
    let colors = debug_colors(ui);
    let breakpoint_color = color32(colors.breakpoint);
    let watchpoint_color = color32(colors.watchpoint);
    let symbol_color = color32(colors.symbol);
    let show_symbols = ui.available_width() >= 430.0;

    ui.heading("Breakpoints");
    if rom_breakpoints && m68000 {
        ui.weak("ROM offsets stop after an opcode with a known sampled source; state-imported queues may be unknown until refill.");
    }
    if m68000 {
        ui.weak("PC boundaries stop before pending trace or interrupt service.");
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("Address:");
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.input)
                .desired_width(150.0)
                .hint_text("hex, symbol or rom:offset"),
        );
        if ui
            .checkbox(&mut state.breakpoint_one_shot, "One-shot")
            .changed()
            && state.breakpoint_one_shot
        {
            state.breakpoint_use_hit_count = false;
        }
        if ui
            .checkbox(&mut state.breakpoint_use_hit_count, "Hit count")
            .changed()
            && state.breakpoint_use_hit_count
        {
            state.breakpoint_one_shot = false;
        }
        if state.breakpoint_use_hit_count {
            ui.add(
                egui::DragValue::new(&mut state.breakpoint_hit_count)
                    .range(1..=u64::MAX)
                    .speed(1),
            );
        }
        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button("Add").clicked() || enter {
            match resolve_breakpoint_input(&state.input, symbols) {
                Ok(BreakpointInput::Cpu(address)) => {
                    if m68000 && (address > 0xff_ffff || address & 1 != 0) {
                        state.input_error = Some("Enter an even 24-bit PC address".into());
                        return;
                    }
                    if state.breakpoint_use_hit_count {
                        actions.add_breakpoint_after =
                            Some((address, state.breakpoint_hit_count.max(1)));
                    } else if state.breakpoint_one_shot {
                        actions.add_one_shot_breakpoint = Some(address);
                    } else {
                        actions.add_breakpoint = Some(address);
                    }
                    state.input.clear();
                    state.input_error = None;
                }
                Ok(BreakpointInput::Rom(offset)) => {
                    if !rom_breakpoints {
                        state.input_error = Some("Physical ROM breakpoints are unavailable".into());
                    } else if state.breakpoint_use_hit_count {
                        state.input_error = Some(
                            "Hit-count physical ROM breakpoints are not supported yet".to_owned(),
                        );
                    } else if state.breakpoint_one_shot {
                        state.input_error = Some(
                            "One-shot physical ROM breakpoints are not supported yet".to_owned(),
                        );
                    } else {
                        actions.add_rom_breakpoints.push(offset);
                        state.input.clear();
                        state.input_error = None;
                    }
                }
                Err(error) => state.input_error = Some(error),
            }
        }
    });
    if let Some(error) = &state.input_error {
        ui.colored_label(watchpoint_color, error);
    }

    if info.breakpoints.is_empty() {
        ui.weak("No breakpoints set.");
    } else {
        egui::Grid::new("bp_grid")
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Address");
                if show_symbols {
                    ui.strong("Symbol");
                }
                ui.strong("Mode");
                ui.strong("Actions");
                ui.end_row();

                for &address in &info.breakpoints {
                    let hit = info.hit_breakpoint == Some(address);
                    ui.label(
                        egui::RichText::new(format_addr(address))
                            .color(if hit {
                                breakpoint_color
                            } else {
                                ui.visuals().text_color()
                            })
                            .monospace(),
                    );
                    if show_symbols {
                        ui.colored_label(
                            symbol_color,
                            symbols
                                .symbol_name_at_cpu_address(address.into())
                                .unwrap_or("-"),
                        );
                    }
                    if let Some(condition) = info
                        .breakpoint_hit_conditions
                        .iter()
                        .find(|condition| condition.address == address)
                    {
                        ui.colored_label(
                            watchpoint_color,
                            format!("Hit {}/{}", condition.hits, condition.target_hits),
                        );
                    } else if info.one_shot_breakpoints.contains(&address) {
                        ui.colored_label(watchpoint_color, "Once");
                    } else {
                        ui.weak("Keep");
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button("Toggle").clicked() {
                            actions.toggle_breakpoints.push(address);
                        }
                        if ui.small_button("Remove").clicked() {
                            actions.remove_breakpoints.push(address);
                        }
                    });
                    ui.end_row();
                }
            });
    }

    if !info.rom_breakpoints.is_empty() {
        ui.separator();
        ui.heading("ROM Breakpoints");
        egui::Grid::new("rom_bp_grid")
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Offset");
                if show_symbols {
                    ui.strong("Symbol");
                }
                ui.strong("Actions");
                ui.end_row();

                for &offset in &info.rom_breakpoints {
                    let hit = info.hit_rom_breakpoint == Some(offset);
                    ui.label(
                        egui::RichText::new(format!("{offset:06X}"))
                            .color(if hit {
                                breakpoint_color
                            } else {
                                ui.visuals().text_color()
                            })
                            .monospace(),
                    );
                    if show_symbols {
                        ui.colored_label(
                            symbol_color,
                            symbols.symbol_name_at_rom_offset(offset).unwrap_or("-"),
                        );
                    }
                    if ui.small_button("Remove").clicked() {
                        actions.remove_rom_breakpoints.push(offset);
                    }
                    ui.end_row();
                }
            });
    }

    if !info.supported_events.is_empty() {
        ui.separator();
        ui.heading("Event Breakpoints");
        ui.horizontal_wrapped(|ui| {
            for &event in &info.supported_events {
                let mut enabled = info.event_breakpoints.contains(&event);
                if ui.checkbox(&mut enabled, event.label()).changed() {
                    actions.event_breakpoint_changes.push((event, enabled));
                }
            }
        });
        if let Some(event) = info.hit_event {
            ui.colored_label(
                breakpoint_color,
                format!("{} breakpoint hit", event.label()),
            );
        }
    }

    ui.add_enabled_ui(memory_watchpoints, |ui| {
        ui.separator();
        ui.heading("Watchpoints");
        if memory_watchpoints && m68000 {
            ui.weak("M68000 operand/stack accesses; stops after the CPU boundary.");
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Range:");
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.watchpoint_input)
                    .desired_width(170.0)
                    .hint_text("symbol or C000-C0FF"),
            );
            egui::ComboBox::from_id_salt("wp_type_bp_window")
                .width(80.0)
                .selected_text(watch_type_label(state.watchpoint_type))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.watchpoint_type, WatchType::Read, "Read");
                    ui.selectable_value(&mut state.watchpoint_type, WatchType::Write, "Write");
                    ui.selectable_value(&mut state.watchpoint_type, WatchType::ReadWrite, "R/W");
                });
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Add").clicked() || enter {
                match parse_watch_range(&state.watchpoint_input, symbols) {
                    Ok((start, end)) => {
                        if m68000 && (start > 0xff_ffff || end > 0xff_ffff) {
                            state.watchpoint_error = Some("Enter 24-bit byte addresses".into());
                        } else {
                            actions.add_watchpoint = Some((start, end, state.watchpoint_type));
                            state.watchpoint_input.clear();
                            state.watchpoint_error = None;
                        }
                    }
                    Err(error) => state.watchpoint_error = Some(error),
                }
            }
        });
        if let Some(error) = &state.watchpoint_error {
            ui.colored_label(watchpoint_color, error);
        }

        if info.watchpoints.is_empty() {
            ui.weak("No watchpoints set.");
        } else {
            egui::Grid::new("wp_grid")
                .striped(true)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.strong("Range");
                    if show_symbols {
                        ui.strong("Symbol");
                    }
                    ui.strong("Type");
                    ui.strong("Actions");
                    ui.end_row();

                    for watch in &info.watchpoints {
                        let hit = info.hit_watchpoint.as_ref().is_some_and(|hit| {
                            (watch.address..=watch.end_address).contains(&hit.address)
                        });
                        ui.label(
                            egui::RichText::new(format_watch_range(
                                watch.address,
                                watch.end_address,
                            ))
                            .color(if hit {
                                watchpoint_color
                            } else {
                                ui.visuals().text_color()
                            })
                            .monospace(),
                        );
                        if show_symbols {
                            ui.colored_label(
                                symbol_color,
                                watch_symbol_label(symbols, watch.address, watch.end_address),
                            );
                        }
                        ui.colored_label(watchpoint_color, watch_type_label(watch.watch_type));
                        if ui.small_button("Remove").clicked() {
                            actions.remove_watchpoints.push((
                                watch.address,
                                watch.end_address,
                                watch.watch_type,
                            ));
                        }
                        ui.end_row();
                    }
                });
        }
    });

    if let Some(address) = info.hit_breakpoint {
        ui.separator();
        ui.colored_label(
            breakpoint_color,
            format!("Breakpoint hit at {}", format_addr(address)),
        );
    }
    if let Some(hit) = &info.hit_watchpoint {
        ui.separator();
        ui.colored_label(
            watchpoint_color,
            format!(
                "Watchpoint {} at {}: {}",
                watch_type_label(hit.watch_type),
                format_addr(hit.address),
                hit.value_text()
            ),
        );
    }

    if info.cpu_state.eq_ignore_ascii_case("Suspended") {
        ui.separator();
        let button = egui::Button::new("Continue (F5)").fill(COLOR_CONTINUE_BUTTON);
        if ui.add(button).clicked() {
            actions.continue_requested = true;
        }
        if ui.button("Step (F7)").clicked() {
            actions.step_requested = true;
        }
        if ui.button("Next Frame").clicked() {
            actions.next_frame_requested = true;
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum BreakpointInput {
    Cpu(Address),
    Rom(u64),
}

fn resolve_breakpoint_input(
    value: &str,
    symbols: &SymbolSession,
) -> Result<BreakpointInput, String> {
    let value = value.trim();
    if let Some(offset) = value.strip_prefix("rom:") {
        return parse_hex_u64(offset)
            .map(BreakpointInput::Rom)
            .ok_or_else(|| "Enter a physical ROM offset after rom:".into());
    }
    if let Some(address) = parse_hex_u32(value) {
        return Ok(BreakpointInput::Cpu(address));
    }
    let matches = symbol_matches(symbols, value);
    if matches.is_empty() {
        return Err(format!("Unknown symbol: {value}"));
    }
    resolve_breakpoint_matches(value, &matches, symbols.exec_mode() == ExecMode::Sm83)
}

fn resolve_breakpoint_matches(
    value: &str,
    matches: &[&SymbolRecord],
    prefer_storage: bool,
) -> Result<BreakpointInput, String> {
    if prefer_storage {
        let mut offsets = Vec::new();
        for symbol in matches {
            if let Some(storage) = symbol.location.storage
                && !offsets.contains(&storage.offset)
            {
                offsets.push(storage.offset);
            }
        }
        match offsets.as_slice() {
            [offset] => return Ok(BreakpointInput::Rom(*offset)),
            [_, _, ..] => return Err(format!("Ambiguous ROM symbol: {value}")),
            [] => {}
        }
    }
    resolve_unique_cpu(value, matches).map(BreakpointInput::Cpu)
}

fn parse_watch_range(value: &str, symbols: &SymbolSession) -> Result<(Address, Address), String> {
    let value = value.trim();
    let (start, end) = value
        .split_once("..")
        .or_else(|| value.split_once('-'))
        .unwrap_or((value, value));
    let start = resolve_cpu_input(start.trim(), symbols)?;
    let end = resolve_cpu_input(end.trim(), symbols)?;
    Ok((start.min(end), start.max(end)))
}

fn resolve_cpu_input(value: &str, symbols: &SymbolSession) -> Result<Address, String> {
    if let Some(address) = parse_hex_u32(value) {
        return Ok(address);
    }
    let matches = symbol_matches(symbols, value);
    resolve_unique_cpu(value, &matches)
}

fn resolve_unique_cpu(value: &str, matches: &[&SymbolRecord]) -> Result<Address, String> {
    let mut addresses = Vec::new();
    for symbol in matches {
        if let Some(cpu) = symbol.location.cpu
            && let Ok(address) = Address::try_from(cpu.address)
            && !addresses.contains(&address)
        {
            addresses.push(address);
        }
    }
    match addresses.as_slice() {
        [address] => Ok(*address),
        [] => Err(format!("Symbol has no CPU address: {value}")),
        [_, _, ..] => Err(format!("Ambiguous CPU symbol: {value}")),
    }
}

fn symbol_matches<'a>(symbols: &'a SymbolSession, value: &str) -> Vec<&'a SymbolRecord> {
    let exact = symbols.store.lookup_name(value).collect::<Vec<_>>();
    if exact.is_empty() {
        symbols.store.lookup_name_case_insensitive(value).collect()
    } else {
        exact
    }
}

fn format_watch_range(start: Address, end: Address) -> String {
    if start == end {
        format_addr(start)
    } else {
        format!("{}-{}", format_addr(start), format_addr(end))
    }
}

fn watch_symbol_label(symbols: &SymbolSession, start: Address, end: Address) -> String {
    let start_name = symbols.symbol_name_at_cpu_address(start.into());
    let end_name = symbols.symbol_name_at_cpu_address(end.into());
    match (start_name, end_name, start == end) {
        (Some(name), _, true) => name.to_owned(),
        (Some(start_name), Some(end_name), false) => format!("{start_name} - {end_name}"),
        (Some(start_name), None, false) => format!("{start_name} + {}", end - start),
        _ => "-".to_owned(),
    }
}

fn watch_type_label(watch_type: WatchType) -> &'static str {
    match watch_type {
        WatchType::Read => "Read",
        WatchType::Write => "Write",
        WatchType::ReadWrite => "R/W",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::{
        AddressSpaceId, Confidence, CpuLocation, ImageId, Provenance, ProvenanceKind, RegionId,
        StorageLocation, SymbolId, SymbolKind, SymbolLocation, SymbolScope,
    };

    #[test]
    fn watch_range_accepts_single_and_ordered_bounds() {
        let symbols = SymbolSession::default();
        assert_eq!(parse_watch_range("C000", &symbols), Ok((0xC000, 0xC000)));
        assert_eq!(
            parse_watch_range("C0FF-C000", &symbols),
            Ok((0xC000, 0xC0FF))
        );
        assert_eq!(
            parse_watch_range("02000000..0200001F", &symbols),
            Ok((0x0200_0000, 0x0200_001F))
        );
    }

    #[test]
    fn symbol_breakpoint_prefers_unique_storage() {
        let symbol = test_symbol("Update", 0x4560, Some(0x8560));
        assert_eq!(
            resolve_breakpoint_matches("Update", &[&symbol], true),
            Ok(BreakpointInput::Rom(0x8560))
        );
        assert_eq!(
            resolve_breakpoint_matches("Update", &[&symbol], false),
            Ok(BreakpointInput::Cpu(0x4560))
        );
    }

    #[test]
    fn watch_range_resolves_symbols() {
        let mut symbols = SymbolSession::default();
        symbols.store.insert(test_symbol("Start", 0xC000, None));
        symbols.store.insert(test_symbol("End", 0xC00F, None));
        assert_eq!(
            parse_watch_range("Start..End", &symbols),
            Ok((0xC000, 0xC00F))
        );
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn md_imported_cpu_symbols_resolve_normal_breakpoint_and_watchpoint_inputs() {
        use crate::symbols::import::{ImportContext, TargetInfo, import_symbols};
        let module = import_symbols(
            "game.sym",
            b"00123400 T Entry\n00FF8040 B Counter\n00FF8041 B CounterEnd",
            &ImportContext {
                target: TargetInfo {
                    system: zeff_emu_common::system::System::Md,
                },
                image: ImageId(0),
                rom_region: RegionId(0),
                cpu_space: AddressSpaceId(0),
                source_name: None,
            },
        )
        .unwrap();
        let mut symbols = SymbolSession::default();
        symbols.store.extend(module.symbols);
        assert_eq!(
            resolve_breakpoint_input("Entry", &symbols),
            Ok(BreakpointInput::Cpu(0x123400))
        );
        assert_eq!(
            parse_watch_range("counter..CounterEnd", &symbols),
            Ok((0xFF8040, 0xFF8041))
        );
        assert_eq!(
            watch_symbol_label(&symbols, 0xFF8040, 0xFF8041),
            "Counter - CounterEnd"
        );
        assert_eq!(format_watch_range(0xFF8040, 0xFF8041), "00FF8040-00FF8041");
    }

    fn test_symbol(name: &str, address: u64, offset: Option<u64>) -> SymbolRecord {
        SymbolRecord {
            id: SymbolId(0),
            name: name.into(),
            location: SymbolLocation {
                cpu: Some(CpuLocation {
                    space: AddressSpaceId(0),
                    address,
                }),
                storage: offset.map(|offset| StorageLocation {
                    image: ImageId(0),
                    region: RegionId(0),
                    offset,
                }),
                bank: None,
                exec_mode: ExecMode::Sm83,
            },
            value: None,
            size: None,
            kind: SymbolKind::Function,
            scope: SymbolScope::Global,
            provenance: Provenance {
                kind: ProvenanceKind::Build,
                source: None,
            },
            confidence: Confidence::Exact,
            comment: None,
        }
    }
}
