use super::*;
use zeff_emu_common::system::System;

fn view(pc: u32) -> crate::debug::DisassemblyView {
    crate::debug::DisassemblyView {
        cursor: crate::debug::DisassemblyCursor::Cpu(pc),
        mapping: None,
        is_navigation_target: true,
        is_static_target: false,
        location_symbol: None,
        lines: vec![crate::debug::DisassembledLine {
            decode_status: None,
            address: pc,
            storage_offset: None,
            bank: None,
            symbol: None,
            control_target: None,
            control_target_storage: None,
            control_target_bank: None,
            control_target_symbol: None,
            source: None,
            bytes: Default::default(),
            mnemonic: Default::default(),
        }],
        breakpoints: Vec::new(),
        one_shot_breakpoints: Vec::new(),
        rom_breakpoints: Vec::new(),
        hit_rom_breakpoint: None,
    }
}

#[test]
fn md_dwarf_sidecar_navigates_cpu_lines_and_loads_source_headlessly() {
    let directory = tempfile::tempdir().unwrap();
    let rom = directory.path().join("game.md");
    let elf = directory.path().join("game.elf");
    let source = directory.path().join("src/main.asm");
    std::fs::create_dir(source.parent().unwrap()).unwrap();
    let text = (1..=12)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    std::fs::write(&source, &text).unwrap();
    std::fs::write(&elf, crate::symbols::import::elf::md_dwarf_fixture()).unwrap();
    for selected in [false, true] {
        let session = if selected {
            SymbolSession::load_for_paths_with_sidecar(System::Md, &rom, &rom, [7; 32], &elf)
        } else {
            SymbolSession::load_for_paths(System::Md, &rom, &rom, [7; 32])
        };
        assert_eq!(session.resolve_cpu_name("Entry"), Some(0x400));
        assert_eq!(session.modules[0].source_line_count, 2);
        assert_eq!(session.modules[0].identity.label(), "Unverified");
        assert!(session.modules[0].diagnostics.is_empty());
        let mut disassembly = view(0x402);
        session.annotate_disassembly(&mut disassembly);
        assert_eq!(
            disassembly.lines[0].source.as_deref(),
            Some("./src/main.asm:10")
        );
        assert_eq!(disassembly.lines[0].storage_offset, None);
        let reference = session
            .source_reference_for_disassembly(&disassembly)
            .unwrap();
        assert_eq!(
            reference.path.canonicalize().unwrap(),
            source.canonicalize().unwrap()
        );
        assert_eq!(reference.line, 10);
        assert_eq!(reference.crc32, None);
        assert_eq!(
            session.source_breakpoint_addresses(reference.source_file, 10),
            [0x400]
        );
        assert!(
            session
                .source_breakpoint_offsets(reference.source_file, 10)
                .is_empty()
        );
        assert!(session.source_reference_at_rom_offset(0x402).is_none());
        let mut state = SourceViewerState::default();
        load_source(&mut state, &reference);
        assert_eq!(state.selected_line, Some(10));
        assert_eq!(state.loaded_source_file, Some(reference.source_file));
        assert_eq!(state.loaded_path.as_ref(), Some(&reference.path));
        assert_eq!(state.lines.len(), 12);
        assert_eq!(state.lines[9], "line 10");
        assert_eq!(
            session
                .source_reference_for_disassembly(&view(0x404))
                .unwrap()
                .line,
            11
        );
        assert!(
            session
                .source_reference_for_disassembly(&view(0x408))
                .is_none()
        );
    }
}
