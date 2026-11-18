use super::*;
use crate::debug::{DisassembledLine, DisassemblyCursor, DisassemblyTarget};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_console_rom_disassembly_produces_source_targets_and_storage_only_labels() {
    let mut backend = backend();
    let symbols = SymbolSession::load_for_backend(&backend);
    let mut state = DebugConsoleState::default();
    for offset in [0x400, 0x1000041, u64::MAX] {
        let actions = command(
            &mut state,
            &symbols,
            &mut backend,
            &format!("disasm rom:{offset:X}"),
        );
        assert_eq!(
            actions.disasm_target,
            Some(DisassemblyTarget::RomStorage(offset))
        );
        assert_eq!(actions.focus_tab, Some(DebugTab::Disassembler));
        assert!(actions.add_one_shot_breakpoint.is_none() && actions.toggle_breakpoints.is_empty());
    }
    for text in [
        "disasm",
        "disasm 400",
        "disasm rom:10000000000000000",
        "disasm rom:FFFFFFFFFFFFFFFFF",
        "disasm rom:400 extra",
    ] {
        assert!(
            command(&mut state, &symbols, &mut backend, text)
                .disasm_target
                .is_none(),
            "{text}"
        );
    }
    for text in ["disasm rom:0x400", "disasm rom:0X400"] {
        assert_eq!(
            command(&mut state, &symbols, &mut backend, text).disasm_target,
            Some(DisassemblyTarget::RomStorage(0x400))
        );
    }
    assert!(
        command(
            &mut state,
            &SymbolSession::default(),
            &mut backend,
            "disasm rom:400"
        )
        .disasm_target
        .is_none()
    );
    assert_eq!(completions("disa", &symbols), ["disasm"]);
    let mut view = crate::debug::DisassemblyView::unavailable_rom(0x400);
    view.lines.push(DisassembledLine {
        address: 0x400,
        decode_status: None,
        storage_offset: Some(0x400),
        bank: None,
        symbol: None,
        control_target: None,
        control_target_storage: None,
        control_target_bank: None,
        control_target_symbol: None,
        source: None,
        bytes: [0x70, 7].into_iter().collect(),
        mnemonic: "MOVEQ #7,D0".try_into().unwrap(),
    });
    let mut memory = MemoryViewerState::new();
    let mut rom = RomViewerState::new();
    let mut actions = DebugUiActions::none();
    for (text, valid) in [("label OriginalCode", true), ("label 400 CpuLabel", false)] {
        run_command(
            &mut state,
            text,
            CommandContext {
                supports_rom_breakpoints: false,
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
        let draft = actions.user_symbol.take().unwrap();
        if valid {
            assert!(draft.location.cpu.is_none());
            assert_eq!(draft.location.storage.unwrap().offset, 0x400);
        } else {
            assert_eq!(draft.location.cpu.unwrap().address, 0x400);
            assert!(draft.location.storage.is_none());
        }
    }
    view.lines.clear();
    run_command(
        &mut state,
        "label Missing",
        CommandContext {
            supports_rom_breakpoints: true,
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
    assert!(actions.user_symbol.is_none());
    assert_eq!(view.cursor, DisassemblyCursor::RomStorage(0x400));
}
