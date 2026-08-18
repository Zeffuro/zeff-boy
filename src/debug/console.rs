use std::collections::VecDeque;

use crate::debug::{
    CpuDebugSnapshot, DebugTab, DebugUiActions, DisassemblyTarget, DisassemblyView,
    MemoryViewerState, RomDebugInfo, RomViewerState,
};
use crate::symbols::{
    AddressSpaceId, CpuLocation, ExecMode, ImageId, RegionId, StorageLocation, SymbolKind,
    SymbolLocation, SymbolRecord, SymbolSession, UserSymbolDraft,
};
use zeff_emu_common::address::Address;

const OUTPUT_LIMIT: usize = 500;
const HISTORY_LIMIT: usize = 100;
const READ_LIMIT: usize = 64;
const COMMANDS: [&str; 20] = [
    "help",
    "clear",
    "find",
    "symbol",
    "peek",
    "romread",
    "goto",
    "mem",
    "rom",
    "break",
    "status",
    "mapper",
    "label",
    "comment",
    "breakonce",
    "breakafter",
    "breakevent",
    "nextframe",
    "call",
    "callundo",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConsoleReadSpace {
    Cpu,
    Rom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PendingConsoleRead {
    pub(crate) space: ConsoleReadSpace,
    pub(crate) start: u32,
    pub(crate) length: usize,
}

pub(crate) struct DebugConsoleState {
    input: String,
    output: VecDeque<String>,
    history: Vec<String>,
    history_index: Option<usize>,
    pub(crate) pending_read: Option<PendingConsoleRead>,
    guest_call_pending: bool,
    guest_call_undo: Option<Vec<u8>>,
}

impl Default for DebugConsoleState {
    fn default() -> Self {
        let mut output = VecDeque::new();
        output.push_back("Debug Console - type help for commands".to_owned());
        Self {
            input: String::new(),
            output,
            history: Vec::new(),
            history_index: None,
            pending_read: None,
            guest_call_pending: false,
            guest_call_undo: None,
        }
    }
}

pub(super) struct DebugConsoleViews<'a> {
    pub(super) memory: &'a mut MemoryViewerState,
    pub(super) rom: &'a mut RomViewerState,
}

pub(super) struct DebugConsoleContext<'a> {
    pub(super) symbols: &'a SymbolSession,
    pub(super) cpu_debug: Option<&'a CpuDebugSnapshot>,
    pub(super) rom_debug: Option<&'a RomDebugInfo>,
    pub(super) disassembly: Option<&'a DisassemblyView>,
    pub(super) memory_page: Option<&'a [(Address, u8)]>,
    pub(super) rom_page: Option<&'a [(u32, u8)]>,
}

struct CommandContext<'a> {
    symbols: &'a SymbolSession,
    cpu_debug: Option<&'a CpuDebugSnapshot>,
    rom_debug: Option<&'a RomDebugInfo>,
    disassembly: Option<&'a DisassemblyView>,
    views: DebugConsoleViews<'a>,
    actions: &'a mut DebugUiActions,
}

pub(super) fn draw_debug_console_content(
    ui: &mut egui::Ui,
    state: &mut DebugConsoleState,
    context: DebugConsoleContext<'_>,
    views: DebugConsoleViews<'_>,
    actions: &mut DebugUiActions,
) {
    complete_pending_read(state, context.memory_page, context.rom_page);

    let output_height = (ui.available_height() - 58.0).max(80.0);
    egui::ScrollArea::vertical()
        .max_height(output_height)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in &state.output {
                ui.monospace(line);
            }
        });
    ui.separator();

    let suggestions = completions(&state.input, context.symbols);
    let mut submit = false;
    ui.horizontal(|ui| {
        ui.monospace(">");
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.input)
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Monospace),
        );
        submit = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if response.has_focus() {
            if ui.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                history_previous(state);
            } else if ui.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                history_next(state);
            } else if ui.input(|input| input.key_pressed(egui::Key::Tab))
                && let Some(first) = suggestions.first()
            {
                state.input.clone_from(first);
            }
        }
        submit |= ui.button("Run").clicked();
    });

    if !suggestions.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for suggestion in suggestions.iter().take(8) {
                if ui.small_button(suggestion).clicked() {
                    state.input.clone_from(suggestion);
                }
            }
        });
    }

    if submit {
        let input = state.input.trim().to_owned();
        state.input.clear();
        run_command(
            state,
            &input,
            CommandContext {
                symbols: context.symbols,
                cpu_debug: context.cpu_debug,
                rom_debug: context.rom_debug,
                disassembly: context.disassembly,
                views,
                actions,
            },
        );
    }
}

fn run_command(state: &mut DebugConsoleState, input: &str, context: CommandContext<'_>) {
    if input.is_empty() {
        return;
    }
    state.push(format!("> {input}"));
    if state.history.last().is_none_or(|last| last != input) {
        state.history.push(input.to_owned());
        if state.history.len() > HISTORY_LIMIT {
            state.history.remove(0);
        }
    }
    state.history_index = None;

    let mut parts = input.split_whitespace();
    let command = parts.next().unwrap_or_default().to_ascii_lowercase();
    let args = parts.collect::<Vec<_>>();
    match command.as_str() {
        "help" => show_help(state),
        "clear" => state.output.clear(),
        "find" => find_symbols(state, context.symbols, &args.join(" ")),
        "symbol" => show_symbols(state, context.symbols, &args.join(" ")),
        "peek" => queue_read(
            state,
            context.symbols,
            &args,
            ConsoleReadSpace::Cpu,
            context.views,
        ),
        "romread" => queue_read(
            state,
            context.symbols,
            &args,
            ConsoleReadSpace::Rom,
            context.views,
        ),
        "goto" => goto_target(
            state,
            context.symbols,
            args.first().copied(),
            context.views,
            context.actions,
        ),
        "mem" => open_memory(
            state,
            context.symbols,
            args.first().copied(),
            context.views,
            context.actions,
        ),
        "rom" => open_rom(
            state,
            context.symbols,
            args.first().copied(),
            context.views,
            context.actions,
        ),
        "break" => toggle_breakpoint(
            state,
            context.symbols,
            args.first().copied(),
            context.actions,
        ),
        "breakonce" => add_one_shot_breakpoint(
            state,
            context.symbols,
            args.first().copied(),
            context.actions,
        ),
        "breakafter" => add_hit_count_breakpoint(
            state,
            context.symbols,
            args.first().copied(),
            args.get(1).copied(),
            context.actions,
        ),
        "breakevent" => toggle_event_breakpoint(
            state,
            context.cpu_debug,
            args.first().copied(),
            context.actions,
        ),
        "nextframe" => {
            context.actions.next_frame_requested = true;
            state.push("Running to next frame".to_owned());
        }
        "call" => queue_guest_call(
            state,
            context.symbols,
            context.cpu_debug,
            &args,
            context.actions,
        ),
        "callundo" => queue_guest_call_undo(state, context.actions),
        "label" => queue_label(
            state,
            context.symbols,
            context.disassembly,
            &args,
            context.actions,
        ),
        "comment" => queue_comment(state, context.symbols, &args, context.actions),
        "status" | "mapper" => show_status(state, context.cpu_debug, context.rom_debug),
        _ => state.push(format!("Unknown command: {command}")),
    }
}

fn show_help(state: &mut DebugConsoleState) {
    for line in [
        "find <text>             Search symbols",
        "symbol <name>          Show exact symbol matches",
        "peek <addr|symbol> [n] Read CPU memory",
        "romread <off|symbol> [n] Read physical ROM",
        "goto <symbol>          Open code or memory",
        "mem <addr|symbol>      Open Memory Viewer",
        "rom <off|symbol>       Open ROM Viewer",
        "break <addr|symbol>    Toggle breakpoint",
        "breakonce <addr|symbol> Break once",
        "breakafter <addr|symbol> <hits> Break after N hits",
        "breakevent <interrupt|dma> Toggle event breakpoint",
        "nextframe             Run to next frame",
        "call <symbol> [limit] Execute a function while suspended",
        "callundo              Undo the last successful call",
        "label <name>           Label current code location",
        "label <addr> <name>    Label a CPU address",
        "comment <symbol> <text> Add or replace a comment",
        "status                 Show CPU and mapper state",
        "clear                  Clear output",
    ] {
        state.push(line.to_owned());
    }
}

fn queue_guest_call(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    cpu: Option<&CpuDebugSnapshot>,
    args: &[&str],
    actions: &mut DebugUiActions,
) {
    let Some(name) = args.first() else {
        state.push("Usage: call <symbol> [instruction limit]".to_owned());
        return;
    };
    if state.guest_call_pending {
        state.push("A guest call is already running".to_owned());
        return;
    }
    if !cpu.is_some_and(|cpu| cpu.cpu_state.eq_ignore_ascii_case("Suspended")) {
        state.push("Suspend the CPU before calling guest code".to_owned());
        return;
    }
    let budget = match args.get(1) {
        Some(value) => match value.parse::<u64>() {
            Ok(value) if (1..=1_000_000).contains(&value) => value,
            _ => {
                state.push("Instruction limit must be 1 to 1000000".to_owned());
                return;
            }
        },
        None => 100_000,
    };
    let mut candidates = exact_symbols(symbols, name)
        .into_iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Label))
        .filter_map(|symbol| {
            let storage = symbol.location.storage;
            let overlay = storage.and_then(|storage| {
                symbols
                    .active_runtime_cpu_for_storage(storage)
                    .map(|cpu| cpu.address)
            });
            let target = overlay
                .or_else(|| symbol.location.cpu.map(|cpu| cpu.address))
                .or_else(|| {
                    storage.and_then(|storage| {
                        symbols
                            .runtime_cpu_for_storage(storage)
                            .map(|cpu| cpu.address)
                    })
                })?;
            Some((symbol, target, overlay.is_some()))
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|(symbol, target, overlay)| {
        (*target, symbol.location.exec_mode, *overlay)
    });
    candidates
        .dedup_by_key(|(symbol, target, overlay)| (*target, symbol.location.exec_mode, *overlay));
    let [(symbol, target, explicit_overlay)] = candidates.as_slice() else {
        state.push(if candidates.is_empty() {
            format!("Could not resolve callable symbol {name}")
        } else {
            format!("Ambiguous callable symbol {name}")
        });
        return;
    };
    let Ok(target) = u32::try_from(*target) else {
        state.push(format!("{} is outside the CPU address space", symbol.name));
        return;
    };
    actions.guest_call = Some(crate::emu_thread::GuestCallRequest {
        name: symbol.name.clone(),
        target,
        storage_offset: symbol.location.storage.map(|storage| storage.offset),
        explicit_overlay: *explicit_overlay,
        exec_mode: symbol.location.exec_mode,
        instruction_budget: budget,
    });
    state.guest_call_pending = true;
    state.push(format!("Calling {} at ${target:X}", symbol.name));
}

fn queue_guest_call_undo(state: &mut DebugConsoleState, actions: &mut DebugUiActions) {
    if state.guest_call_pending {
        state.push("Wait for the current guest call".to_owned());
    } else if let Some(saved) = &state.guest_call_undo {
        actions.undo_guest_call = Some(saved.clone());
        state.guest_call_pending = true;
        state.push("Restoring pre-call state".to_owned());
    } else {
        state.push("No guest call to undo".to_owned());
    }
}

fn queue_comment(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    args: &[&str],
    actions: &mut DebugUiActions,
) {
    let [name, comment @ ..] = args else {
        state.push("Usage: comment <symbol> <text>".to_owned());
        return;
    };
    if comment.is_empty() {
        state.push("Usage: comment <symbol> <text>".to_owned());
        return;
    }
    let Some(symbol) = exact_symbols(symbols, name).into_iter().next() else {
        state.push(format!("Could not resolve {name}"));
        return;
    };
    actions.user_symbol = Some(UserSymbolDraft {
        name: symbol.name.clone(),
        location: symbol.location,
        value: symbol.value,
        kind: symbol.kind,
        size: symbol.size,
        comment: Some(comment.join(" ")),
    });
    state.push(format!("Adding comment to {}", symbol.name));
}

fn queue_label(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    disassembly: Option<&DisassemblyView>,
    args: &[&str],
    actions: &mut DebugUiActions,
) {
    let (address, name) = match args {
        [] => {
            state.push("Usage: label <name> or label <address> <name>".to_owned());
            return;
        }
        [name] => {
            let Some(view) = disassembly else {
                state.push("Current code location is unavailable".to_owned());
                return;
            };
            (u64::from(view.pc), (*name).to_owned())
        }
        [value, name @ ..] => {
            let Some(address) = resolve_cpu(value, symbols) else {
                state.push(format!("Could not resolve {value}"));
                return;
            };
            (address, name.join(" "))
        }
    };
    let storage_offset = disassembly.and_then(|view| {
        view.lines
            .iter()
            .find(|line| u64::from(line.address) == address)
            .and_then(|line| line.storage_offset)
    });
    let exec_mode = symbols.exec_mode();
    actions.user_symbol = Some(UserSymbolDraft {
        name: name.clone(),
        location: SymbolLocation {
            cpu: Some(CpuLocation {
                space: AddressSpaceId(0),
                address,
            }),
            storage: storage_offset.map(|offset| StorageLocation {
                image: ImageId(0),
                region: RegionId(0),
                offset,
            }),
            bank: (exec_mode == ExecMode::Sm83)
                .then(|| storage_offset.map(|offset| (offset / 0x4000) as u32))
                .flatten(),
            exec_mode,
        },
        value: None,
        kind: SymbolKind::Label,
        size: None,
        comment: None,
    });
    state.push(format!("Adding {name} at CPU ${address:X}"));
}

fn find_symbols(state: &mut DebugConsoleState, symbols: &SymbolSession, query: &str) {
    if query.trim().is_empty() {
        state.push("Usage: find <text>".to_owned());
        return;
    }
    let ids = symbols.store.search_ids(query, 25);
    if ids.is_empty() {
        state.push("No symbols found".to_owned());
        return;
    }
    for id in ids {
        if let Some(symbol) = symbols.store.symbol(id) {
            state.push(format_symbol(symbol));
        }
    }
}

fn show_symbols(state: &mut DebugConsoleState, symbols: &SymbolSession, name: &str) {
    let matches = exact_symbols(symbols, name);
    if matches.is_empty() {
        state.push("No exact symbol match".to_owned());
        return;
    }
    for symbol in matches {
        state.push(format_symbol(symbol));
    }
}

fn queue_read(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    args: &[&str],
    space: ConsoleReadSpace,
    views: DebugConsoleViews<'_>,
) {
    let Some(value) = args.first() else {
        state.push("Missing address or symbol".to_owned());
        return;
    };
    let address = match space {
        ConsoleReadSpace::Cpu => resolve_cpu(value, symbols),
        ConsoleReadSpace::Rom => resolve_rom(value, symbols),
    };
    let Some(start) = address.and_then(|address| u32::try_from(address).ok()) else {
        state.push(format!("Could not resolve {value}"));
        return;
    };
    let length = args
        .get(1)
        .and_then(|length| length.parse::<usize>().ok())
        .unwrap_or(16)
        .clamp(1, READ_LIMIT);
    state.pending_read = Some(PendingConsoleRead {
        space,
        start,
        length,
    });
    match space {
        ConsoleReadSpace::Cpu => {
            views.memory.view_start = views.memory.address_space.clamp_start(start);
            views.memory.jump_input = views.memory.address_space.format(views.memory.view_start);
        }
        ConsoleReadSpace::Rom => {
            views.rom.view_start = start;
            views.rom.jump_input = format!("{start:06X}");
        }
    }
    state.push(format!("Reading {length} byte(s) at ${start:X}…"));
}

fn goto_target(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    value: Option<&str>,
    views: DebugConsoleViews<'_>,
    actions: &mut DebugUiActions,
) {
    let Some(value) = value else {
        state.push("Usage: goto <symbol>".to_owned());
        return;
    };
    if let Some((symbol, cpu_address)) = exact_symbols(symbols, value)
        .into_iter()
        .filter(|symbol| symbol.location.storage.is_some())
        .find_map(|symbol| {
            let cpu = symbol
                .location
                .storage
                .and_then(|storage| symbols.runtime_cpu_for_storage(storage))
                .or(symbol.location.cpu)?;
            Some((symbol, cpu.address))
        })
    {
        let storage_offset = symbol.location.storage.unwrap().offset;
        if let Ok(cpu_address) = u32::try_from(cpu_address) {
            actions.disasm_target = Some(DisassemblyTarget {
                cpu_address,
                storage_offset: Some(storage_offset),
                thumb: match symbol.location.exec_mode {
                    ExecMode::Thumb => Some(true),
                    ExecMode::Arm => Some(false),
                    _ => None,
                },
            });
            actions.focus_tab = Some(DebugTab::Disassembler);
            state.push(format!("Opened {} in Disassembler", symbol.name));
            return;
        }
    }
    open_memory(state, symbols, Some(value), views, actions);
}

fn open_memory(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    value: Option<&str>,
    views: DebugConsoleViews<'_>,
    actions: &mut DebugUiActions,
) {
    let Some(value) = value else {
        state.push("Usage: mem <address|symbol>".to_owned());
        return;
    };
    let Some(address) = resolve_cpu(value, symbols).and_then(|value| u32::try_from(value).ok())
    else {
        state.push(format!("Could not resolve {value}"));
        return;
    };
    views.memory.view_start = views.memory.address_space.clamp_start(address);
    views.memory.jump_input = views.memory.address_space.format(views.memory.view_start);
    actions.focus_tab = Some(DebugTab::MemoryViewer);
    state.push(format!("Opened CPU ${address:X}"));
}

fn open_rom(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    value: Option<&str>,
    views: DebugConsoleViews<'_>,
    actions: &mut DebugUiActions,
) {
    let Some(value) = value else {
        state.push("Usage: rom <offset|symbol>".to_owned());
        return;
    };
    let Some(offset) = resolve_rom(value, symbols).and_then(|value| u32::try_from(value).ok())
    else {
        state.push(format!("Could not resolve {value}"));
        return;
    };
    views.rom.view_start = offset;
    views.rom.jump_input = format!("{offset:06X}");
    actions.focus_tab = Some(DebugTab::RomViewer);
    state.push(format!("Opened ROM ${offset:X}"));
}

fn toggle_breakpoint(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    value: Option<&str>,
    actions: &mut DebugUiActions,
) {
    let Some(value) = value else {
        state.push("Usage: break <address|symbol>".to_owned());
        return;
    };
    let exact = exact_symbols(symbols, value);
    if symbols.exec_mode() == ExecMode::Sm83 {
        let mut offsets = exact
            .iter()
            .filter_map(|symbol| symbol.location.storage.map(|storage| storage.offset))
            .collect::<Vec<_>>();
        offsets.sort_unstable();
        offsets.dedup();
        match offsets.as_slice() {
            [offset] => {
                actions.toggle_rom_breakpoints.push(*offset);
                state.push(format!("Toggled ROM breakpoint at {value}"));
                return;
            }
            [_, _, ..] => {
                state.push(format!("Ambiguous ROM symbol: {value}"));
                return;
            }
            [] => {}
        }
    }
    let Some(address) = resolve_cpu(value, symbols).and_then(|value| u32::try_from(value).ok())
    else {
        state.push(format!("Could not resolve {value}"));
        return;
    };
    actions.toggle_breakpoints.push(address);
    state.push(format!("Toggled CPU breakpoint at ${address:X}"));
}

fn add_one_shot_breakpoint(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    value: Option<&str>,
    actions: &mut DebugUiActions,
) {
    let Some(value) = value else {
        state.push("Usage: breakonce <address|symbol>".to_owned());
        return;
    };
    if let Some(address) = parse_hex(value).and_then(|value| Address::try_from(value).ok()) {
        actions.add_one_shot_breakpoint = Some(address);
        state.push(format!("Breaking once at CPU ${address:X}"));
        return;
    }

    let exact = exact_symbols(symbols, value);
    if symbols.exec_mode() == ExecMode::Sm83
        && exact.iter().any(|symbol| symbol.location.storage.is_some())
    {
        state.push("One-shot physical ROM breakpoints are not supported yet".to_owned());
        return;
    }
    let mut addresses = exact
        .iter()
        .filter_map(|symbol| symbol.location.cpu)
        .filter_map(|cpu| Address::try_from(cpu.address).ok())
        .collect::<Vec<_>>();
    addresses.sort_unstable();
    addresses.dedup();
    match addresses.as_slice() {
        [address] => {
            actions.add_one_shot_breakpoint = Some(*address);
            state.push(format!("Breaking once at CPU ${address:X}"));
        }
        [] => state.push(format!("Could not resolve {value}")),
        [_, _, ..] => state.push(format!("Ambiguous CPU symbol: {value}")),
    }
}

fn add_hit_count_breakpoint(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    value: Option<&str>,
    hits: Option<&str>,
    actions: &mut DebugUiActions,
) {
    let (Some(value), Some(hits)) = (value, hits) else {
        state.push("Usage: breakafter <address|symbol> <hits>".to_owned());
        return;
    };
    let Ok(hits) = hits.parse::<u64>() else {
        state.push("Hit count must be a positive integer".to_owned());
        return;
    };
    if hits == 0 {
        state.push("Hit count must be a positive integer".to_owned());
        return;
    }
    if let Some(address) = parse_hex(value).and_then(|value| Address::try_from(value).ok()) {
        actions.add_breakpoint_after = Some((address, hits));
        state.push(format!("Breaking at CPU ${address:X} after {hits} hits"));
        return;
    }

    let exact = exact_symbols(symbols, value);
    if symbols.exec_mode() == ExecMode::Sm83
        && exact.iter().any(|symbol| symbol.location.storage.is_some())
    {
        state.push("Hit-count physical ROM breakpoints are not supported yet".to_owned());
        return;
    }
    let mut addresses = exact
        .iter()
        .filter_map(|symbol| symbol.location.cpu)
        .filter_map(|cpu| Address::try_from(cpu.address).ok())
        .collect::<Vec<_>>();
    addresses.sort_unstable();
    addresses.dedup();
    match addresses.as_slice() {
        [address] => {
            actions.add_breakpoint_after = Some((*address, hits));
            state.push(format!("Breaking at CPU ${address:X} after {hits} hits"));
        }
        [] => state.push(format!("Could not resolve {value}")),
        [_, _, ..] => state.push(format!("Ambiguous CPU symbol: {value}")),
    }
}

fn toggle_event_breakpoint(
    state: &mut DebugConsoleState,
    cpu: Option<&CpuDebugSnapshot>,
    value: Option<&str>,
    actions: &mut DebugUiActions,
) {
    let Some(value) = value else {
        state.push("Usage: breakevent <interrupt|dma>".to_owned());
        return;
    };
    let Some(event) = parse_debug_event(value) else {
        state.push(format!("Unknown event: {value}"));
        return;
    };
    let Some(cpu) = cpu else {
        state.push("CPU debug state is unavailable".to_owned());
        return;
    };
    if !cpu.supported_events.contains(&event) {
        state.push(format!(
            "{} breakpoints are unavailable for this core",
            event.label()
        ));
        return;
    }
    let enabled = !cpu.event_breakpoints.contains(&event);
    actions.event_breakpoint_changes.push((event, enabled));
    state.push(format!(
        "{} {} breakpoint",
        if enabled { "Enabled" } else { "Disabled" },
        event.label()
    ));
}

fn parse_debug_event(value: &str) -> Option<zeff_emu_common::debug::DebugEvent> {
    match value.to_ascii_lowercase().as_str() {
        "interrupt" | "irq" | "nmi" => Some(zeff_emu_common::debug::DebugEvent::Interrupt),
        "dma" => Some(zeff_emu_common::debug::DebugEvent::Dma),
        _ => None,
    }
}

fn show_status(
    state: &mut DebugConsoleState,
    cpu_debug: Option<&CpuDebugSnapshot>,
    rom_debug: Option<&RomDebugInfo>,
) {
    if let Some(cpu) = cpu_debug {
        state.push(format!("CPU: {}  cycles {}", cpu.cpu_state, cpu.cycles));
        for line in &cpu.register_lines {
            state.push(line.clone());
        }
    } else {
        state.push("CPU snapshot unavailable".to_owned());
    }
    if let Some(rom) = rom_debug {
        for section in &rom.sections {
            for (label, value) in &section.fields {
                let key = label.to_ascii_lowercase();
                if key.contains("bank") || key.contains("mapper") || key.contains("mbc") {
                    state.push(format!("{label}: {value}"));
                }
            }
        }
    }
}

fn complete_pending_read(
    state: &mut DebugConsoleState,
    memory_page: Option<&[(Address, u8)]>,
    rom_page: Option<&[(u32, u8)]>,
) {
    let Some(pending) = state.pending_read else {
        return;
    };
    let values = match pending.space {
        ConsoleReadSpace::Cpu => memory_page.and_then(|page| {
            page.iter()
                .position(|(address, _)| *address == pending.start)
                .map(|start| {
                    page[start..]
                        .iter()
                        .take(pending.length)
                        .map(|(_, value)| *value)
                        .collect::<Vec<_>>()
                })
        }),
        ConsoleReadSpace::Rom => rom_page.and_then(|page| {
            page.iter()
                .position(|(offset, _)| *offset == pending.start)
                .map(|start| {
                    page[start..]
                        .iter()
                        .take(pending.length)
                        .map(|(_, value)| *value)
                        .collect::<Vec<_>>()
                })
        }),
    };
    let Some(values) = values else {
        return;
    };
    let rendered = values
        .iter()
        .map(|value| format!("{value:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    state.push(format!("${:X}: {rendered}", pending.start));
    state.pending_read = None;
}

fn completions(input: &str, symbols: &SymbolSession) -> Vec<String> {
    let trimmed = input.trim_start();
    let Some((command, argument)) = trimmed.split_once(char::is_whitespace) else {
        let needle = trimmed.to_ascii_lowercase();
        return COMMANDS
            .iter()
            .filter(|command| command.starts_with(&needle))
            .map(|command| (*command).to_owned())
            .collect();
    };
    let argument = argument.trim();
    if argument.is_empty() {
        return Vec::new();
    }
    symbols
        .store
        .search_ids(argument, 8)
        .into_iter()
        .filter_map(|id| symbols.store.symbol(id))
        .map(|symbol| format!("{command} {}", symbol.name))
        .collect()
}

fn exact_symbols<'a>(symbols: &'a SymbolSession, name: &str) -> Vec<&'a SymbolRecord> {
    let mut matches = symbols
        .store
        .lookup_name(name)
        .chain(symbols.store.lookup_name_case_insensitive(name))
        .collect::<Vec<_>>();
    matches.sort_unstable_by_key(|symbol| symbol.id);
    matches.dedup_by_key(|symbol| symbol.id);
    matches
}

fn resolve_cpu(value: &str, symbols: &SymbolSession) -> Option<u64> {
    symbols.resolve_cpu_name(value).or_else(|| parse_hex(value))
}

fn resolve_rom(value: &str, symbols: &SymbolSession) -> Option<u64> {
    symbols.resolve_rom_name(value).or_else(|| parse_hex(value))
}

fn parse_hex(value: &str) -> Option<u64> {
    let value = value
        .strip_prefix('$')
        .or_else(|| value.strip_prefix("0x"))
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value)
        .replace('_', "");
    u64::from_str_radix(&value, 16).ok()
}

fn format_symbol(symbol: &SymbolRecord) -> String {
    let mut locations = Vec::new();
    if let Some(bank) = symbol.location.bank {
        locations.push(format!("bank ${bank:X}"));
    }
    if let Some(cpu) = symbol.location.cpu {
        locations.push(format!("CPU ${:X}", cpu.address));
    }
    if let Some(storage) = symbol.location.storage {
        locations.push(format!("ROM ${:X}", storage.offset));
    }
    if let Some(value) = symbol.value {
        locations.push(format!("value ${value:X}"));
    }
    let mut rendered = format!(
        "{}  {:?}  {}",
        symbol.name,
        symbol.kind,
        locations.join("  ")
    );
    if let Some(comment) = &symbol.comment {
        rendered.push_str("  ; ");
        rendered.push_str(comment);
    }
    rendered
}

fn history_previous(state: &mut DebugConsoleState) {
    if state.history.is_empty() {
        return;
    }
    let index = state
        .history_index
        .map_or(state.history.len() - 1, |index| index.saturating_sub(1));
    state.history_index = Some(index);
    state.input.clone_from(&state.history[index]);
}

fn history_next(state: &mut DebugConsoleState) {
    let Some(index) = state.history_index else {
        return;
    };
    if index + 1 < state.history.len() {
        state.history_index = Some(index + 1);
        state.input.clone_from(&state.history[index + 1]);
    } else {
        state.history_index = None;
        state.input.clear();
    }
}

impl DebugConsoleState {
    pub(crate) fn guest_call_completed(
        &mut self,
        name: &str,
        instructions: u64,
        undo_state: Vec<u8>,
    ) {
        self.guest_call_pending = false;
        self.guest_call_undo = Some(undo_state);
        self.push(format!("{name} returned after {instructions} instructions"));
    }

    pub(crate) fn guest_call_failed(&mut self, name: &str, error: &str) {
        self.guest_call_pending = false;
        self.push(format!("{name} failed: {error}"));
    }

    pub(crate) fn guest_call_undone(&mut self) {
        self.guest_call_pending = false;
        self.guest_call_undo = None;
        self.push("Guest call undone".to_owned());
    }

    pub(crate) fn guest_call_undo_failed(&mut self, error: &str) {
        self.guest_call_pending = false;
        self.push(format!("Could not undo guest call: {error}"));
    }

    fn push(&mut self, line: String) {
        self.output.push_back(line);
        while self.output.len() > OUTPUT_LIMIT {
            self.output.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommandContext, ConsoleReadSpace, DebugConsoleState, DebugConsoleViews, completions,
        history_next, history_previous, parse_debug_event, parse_hex, run_command,
    };
    use crate::debug::{
        CpuDebugSnapshot, DebugTab, DebugUiActions, DisassembledLine, DisassemblyView,
        MemoryViewerState, RomViewerState,
    };
    use crate::symbols::import::{ImportContext, TargetInfo, import_symbols};
    use crate::symbols::{AddressSpaceId, ImageId, RegionId, SymbolSession};

    #[test]
    fn parses_debugger_hex_notation() {
        assert_eq!(parse_hex("$C100"), Some(0xC100));
        assert_eq!(parse_hex("0x8560"), Some(0x8560));
        assert_eq!(parse_hex("12_34"), Some(0x1234));
    }

    #[test]
    fn parses_event_breakpoint_names() {
        use zeff_emu_common::debug::DebugEvent;

        assert_eq!(parse_debug_event("irq"), Some(DebugEvent::Interrupt));
        assert_eq!(parse_debug_event("NMI"), Some(DebugEvent::Interrupt));
        assert_eq!(parse_debug_event("dma"), Some(DebugEvent::Dma));
        assert_eq!(parse_debug_event("scanline"), None);
    }

    fn suspended_cpu() -> CpuDebugSnapshot {
        CpuDebugSnapshot {
            register_lines: Vec::new(),
            flags: Vec::new(),
            status_text: String::new(),
            cpu_state: "Suspended".to_owned(),
            pc: 0,
            cycles: 0,
            last_opcode_line: String::new(),
            sections: Vec::new(),
            io_registers: Vec::new(),
            recent_opcodes: Vec::new(),
            call_stack: Vec::new(),
            call_stack_available: false,
            breakpoints: Vec::new(),
            one_shot_breakpoints: Vec::new(),
            breakpoint_hit_conditions: Vec::new(),
            supported_events: Vec::new(),
            event_breakpoints: Vec::new(),
            rom_breakpoints: Vec::new(),
            watchpoints: Vec::new(),
            hit_breakpoint: None,
            hit_rom_breakpoint: None,
            hit_watchpoint: None,
            hit_event: None,
        }
    }

    #[test]
    fn completes_commands_and_walks_history() {
        let symbols = SymbolSession::default();
        assert_eq!(completions("pe", &symbols), ["peek"]);
        assert_eq!(completions("lab", &symbols), ["label"]);

        let mut state = DebugConsoleState {
            history: vec!["status".to_owned(), "peek C100".to_owned()],
            ..Default::default()
        };
        history_previous(&mut state);
        assert_eq!(state.input, "peek C100");
        history_previous(&mut state);
        assert_eq!(state.input, "status");
        history_next(&mut state);
        assert_eq!(state.input, "peek C100");
    }

    #[test]
    fn symbol_commands_navigate_and_queue_reads() {
        let mut symbols = SymbolSession::default();
        let module = import_symbols(
            "game.sym",
            b"02:4560 UpdatePlayer\n00:C100 PlayerData",
            &ImportContext {
                target: TargetInfo {
                    system: zeff_emu_common::system::System::Gb,
                },
                image: ImageId(0),
                rom_region: RegionId(0),
                cpu_space: AddressSpaceId(0),
                source_name: None,
            },
        )
        .unwrap();
        symbols.store.extend(module.symbols);
        assert_eq!(completions("goto upd", &symbols), ["goto UpdatePlayer"]);

        let mut state = DebugConsoleState::default();
        let mut memory = MemoryViewerState::new();
        let mut rom = RomViewerState::new();
        let mut actions = DebugUiActions::none();
        run_command(
            &mut state,
            "goto UpdatePlayer",
            CommandContext {
                symbols: &symbols,
                cpu_debug: None,
                rom_debug: None,
                disassembly: None,
                views: DebugConsoleViews {
                    memory: &mut memory,
                    rom: &mut rom,
                },
                actions: &mut actions,
            },
        );
        assert_eq!(actions.focus_tab, Some(DebugTab::Disassembler));
        assert_eq!(actions.disasm_target.unwrap().storage_offset, Some(0x8560));

        run_command(
            &mut state,
            "peek PlayerData 4",
            CommandContext {
                symbols: &symbols,
                cpu_debug: None,
                rom_debug: None,
                disassembly: None,
                views: DebugConsoleViews {
                    memory: &mut memory,
                    rom: &mut rom,
                },
                actions: &mut actions,
            },
        );
        let pending = state.pending_read.unwrap();
        assert_eq!(pending.space, ConsoleReadSpace::Cpu);
        assert_eq!(pending.start, 0xC100);
        assert_eq!(pending.length, 4);

        run_command(
            &mut state,
            "breakonce C123",
            CommandContext {
                symbols: &symbols,
                cpu_debug: None,
                rom_debug: None,
                disassembly: None,
                views: DebugConsoleViews {
                    memory: &mut memory,
                    rom: &mut rom,
                },
                actions: &mut actions,
            },
        );
        assert_eq!(actions.add_one_shot_breakpoint, Some(0xC123));

        run_command(
            &mut state,
            "breakafter C124 8",
            CommandContext {
                symbols: &symbols,
                cpu_debug: None,
                rom_debug: None,
                disassembly: None,
                views: DebugConsoleViews {
                    memory: &mut memory,
                    rom: &mut rom,
                },
                actions: &mut actions,
            },
        );
        assert_eq!(actions.add_breakpoint_after, Some((0xC124, 8)));

        run_command(
            &mut state,
            "nextframe",
            CommandContext {
                symbols: &symbols,
                cpu_debug: None,
                rom_debug: None,
                disassembly: None,
                views: DebugConsoleViews {
                    memory: &mut memory,
                    rom: &mut rom,
                },
                actions: &mut actions,
            },
        );
        assert!(actions.next_frame_requested);

        run_command(
            &mut state,
            "comment UpdatePlayer Main movement routine",
            CommandContext {
                symbols: &symbols,
                cpu_debug: None,
                rom_debug: None,
                disassembly: None,
                views: DebugConsoleViews {
                    memory: &mut memory,
                    rom: &mut rom,
                },
                actions: &mut actions,
            },
        );
        assert_eq!(
            actions.user_symbol.unwrap().comment.as_deref(),
            Some("Main movement routine")
        );
    }

    #[test]
    fn guest_call_command_requires_a_symbol_and_queues_a_budget() {
        let mut symbols = SymbolSession::default();
        let module = import_symbols(
            "game.sym",
            b"02:4560 UpdatePlayer",
            &ImportContext {
                target: TargetInfo {
                    system: zeff_emu_common::system::System::Gb,
                },
                image: ImageId(0),
                rom_region: RegionId(0),
                cpu_space: AddressSpaceId(0),
                source_name: None,
            },
        )
        .unwrap();
        symbols.store.extend(module.symbols);
        let cpu = suspended_cpu();
        let mut state = DebugConsoleState::default();
        let mut memory = MemoryViewerState::new();
        let mut rom = RomViewerState::new();
        let mut actions = DebugUiActions::none();
        run_command(
            &mut state,
            "call UpdatePlayer 25",
            CommandContext {
                symbols: &symbols,
                cpu_debug: Some(&cpu),
                rom_debug: None,
                disassembly: None,
                views: DebugConsoleViews {
                    memory: &mut memory,
                    rom: &mut rom,
                },
                actions: &mut actions,
            },
        );

        let call = actions.guest_call.unwrap();
        assert_eq!(call.target, 0x4560);
        assert_eq!(call.storage_offset, Some(0x8560));
        assert_eq!(call.instruction_budget, 25);
    }

    #[test]
    fn labels_the_current_mapped_code_location() {
        let symbols = SymbolSession::default();
        let view = DisassemblyView {
            pc: 0x4560,
            mapping: Some(2),
            is_navigation_target: false,
            is_static_target: false,
            location_symbol: None,
            lines: vec![DisassembledLine {
                address: 0x4560,
                storage_offset: Some(0x8560),
                symbol: None,
                control_target: None,
                control_target_storage: None,
                control_target_symbol: None,
                source: None,
                bytes: Default::default(),
                mnemonic: Default::default(),
            }],
            breakpoints: Vec::new(),
            one_shot_breakpoints: Vec::new(),
            rom_breakpoints: Vec::new(),
            hit_rom_breakpoint: None,
        };
        let mut state = DebugConsoleState::default();
        let mut memory = MemoryViewerState::new();
        let mut rom = RomViewerState::new();
        let mut actions = DebugUiActions::none();
        run_command(
            &mut state,
            "label UpdatePlayer",
            CommandContext {
                symbols: &symbols,
                cpu_debug: None,
                rom_debug: None,
                disassembly: Some(&view),
                views: DebugConsoleViews {
                    memory: &mut memory,
                    rom: &mut rom,
                },
                actions: &mut actions,
            },
        );

        let symbol = actions.user_symbol.unwrap();
        assert_eq!(symbol.name, "UpdatePlayer");
        assert_eq!(symbol.location.cpu.unwrap().address, 0x4560);
        assert_eq!(symbol.location.storage.unwrap().offset, 0x8560);
    }
}
