use super::commands::{CommandContext, completions, run_command};
use super::{ConsoleReadSpace, DebugConsoleState, DebugConsoleViews};
use crate::debug::{DebugTab, DebugUiActions, MemoryViewerState, RomViewerState};
use crate::symbols::import::{ImportContext, TargetInfo, import_symbols};
use crate::symbols::{AddressSpaceId, ImageId, RegionId, SymbolSession};
use zeff_emu_common::system::System;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_imported_symbols_drive_console_navigation_breakpoints_and_read_requests() {
    let module = import_symbols(
        "game.sym",
        b"00123400 T Entry\n00FF8040 B Counter",
        &ImportContext {
            target: TargetInfo { system: System::Md },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: None,
        },
    )
    .unwrap();
    let mut symbols = SymbolSession::default();
    symbols.store.extend(module.symbols);
    assert_eq!(completions("goto ent", &symbols), ["goto Entry"]);
    let mut state = DebugConsoleState::default();
    let mut memory = MemoryViewerState::new();
    memory.configure_for_system(System::Md);
    let mut rom = RomViewerState::new();
    let mut actions = DebugUiActions::none();
    for command in [
        "goto Entry",
        "break Entry",
        "breakonce Entry",
        "breakafter Entry 3",
        "peek Counter 2",
    ] {
        run_command(
            &mut state,
            command,
            CommandContext {
                supports_rom_breakpoints: true,
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
    }
    assert_eq!(actions.focus_tab, Some(DebugTab::Disassembler));
    let target = actions.disasm_target.unwrap().as_cpu().unwrap();
    assert_eq!(target.cpu_address, 0x123400);
    assert!(target.storage_offset.is_none() && target.bank.is_none() && target.thumb.is_none());
    assert_eq!(actions.toggle_breakpoints, [0x123400]);
    assert_eq!(actions.add_one_shot_breakpoint, Some(0x123400));
    assert_eq!(actions.add_breakpoint_after, Some((0x123400, 3)));
    assert!(actions.toggle_rom_breakpoints.is_empty());
    let read = state.pending_read.unwrap();
    assert_eq!(read.space, ConsoleReadSpace::Cpu);
    assert_eq!((read.start, read.length), (0xFF8040, 2));
    assert!(!memory.enable_editing);

    let mut symbol = symbols.store.lookup_name("Entry").next().unwrap().clone();
    symbol.name = "ExplicitStorage".into();
    symbol.location.storage = Some(crate::symbols::StorageLocation {
        image: ImageId(0),
        region: RegionId(0),
        offset: 0x400,
    });
    symbol.location.bank = Some(7);
    symbols.store.insert(symbol);
    run_command(
        &mut state,
        "goto ExplicitStorage",
        CommandContext {
            supports_rom_breakpoints: true,
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
    let target = actions.disasm_target.unwrap().as_cpu().unwrap();
    assert_eq!(target.cpu_address, 0x123400);
    assert!(target.storage_offset.is_none() && target.bank.is_none());
}

mod guest_calls;
