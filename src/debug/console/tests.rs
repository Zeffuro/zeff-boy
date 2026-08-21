use super::commands::{CommandContext, completions, parse_debug_event, parse_hex, run_command};
use super::{
    ConsoleReadSpace, DebugConsoleState, DebugConsoleViews, history_next, history_previous,
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
