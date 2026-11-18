use super::*;

#[test]
fn md_elf_symbols_drive_console_requests_and_real_call_undo_without_storage_guesses() {
    let module = import_symbols(
        "linked.elf",
        &crate::symbols::import::elf::md_elf_fixture(),
        &ImportContext {
            target: TargetInfo { system: System::Md },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: Some("linked.elf".into()),
        },
    )
    .unwrap();
    let mut symbols = SymbolSession::default();
    symbols.store.extend(module.symbols);
    assert_eq!(completions("goto ent", &symbols), ["goto Entry"]);
    assert_eq!(symbols.resolve_rom_name("Entry"), None);
    let mut backend = backend();
    backend.debug_suspend();
    let original = machine(&backend).encode_frontend_state().unwrap();
    let mut state = DebugConsoleState::default();
    let actions = command(&mut state, &symbols, &mut backend, "goto Entry");
    let target = actions.disasm_target.unwrap().as_cpu().unwrap();
    assert_eq!(target.cpu_address, 0x400);
    assert!(target.storage_offset.is_none() && target.bank.is_none() && target.thumb.is_none());
    let actions = command(&mut state, &symbols, &mut backend, "break Entry");
    assert_eq!(actions.toggle_breakpoints, [0x400]);
    assert!(actions.toggle_rom_breakpoints.is_empty());
    let actions = command(&mut state, &symbols, &mut backend, "peek Scratch 2");
    assert!(actions.memory_writes.is_empty());
    let read = state.pending_read.take().unwrap();
    assert_eq!(
        (read.space, read.start, read.length),
        (ConsoleReadSpace::Cpu, 0xff0081, 2)
    );
    for name in ["Counter", "OddData", "Scratch", "End", "Mask"] {
        assert!(
            command(
                &mut state,
                &symbols,
                &mut backend,
                &format!("call {name} 4")
            )
            .guest_call
            .is_none()
        );
        assert!(!state.guest_call_pending);
    }
    assert_eq!(machine(&backend).encode_frontend_state().unwrap(), original);
    let request = command(&mut state, &symbols, &mut backend, "call Entry 4")
        .guest_call
        .unwrap();
    assert_eq!(
        (
            request.target,
            request.exec_mode,
            request.instruction_budget
        ),
        (0x400, ExecMode::M68000, 4)
    );
    assert!(request.storage_offset.is_none() && !request.explicit_overlay);
    let (instructions, token) = backend.execute_guest_call(&request).unwrap();
    assert_eq!((instructions, token.len()), (4, 16));
    state.guest_call_completed(&request.name, instructions, token.clone());
    assert!(!state.guest_call_pending);
    assert_eq!(machine(&backend).registers().d[0], 8);
    assert_eq!(machine(&backend).peek(0xff0040), Some(8));
    assert!(backend.is_suspended());
    let undo = command(&mut state, &symbols, &mut backend, "callundo")
        .undo_guest_call
        .unwrap();
    assert_eq!(undo, token);
    backend.undo_guest_call(undo).unwrap();
    state.guest_call_undone();
    assert_eq!(machine(&backend).encode_frontend_state().unwrap(), original);
    assert!(backend.is_suspended());
    assert!(!state.guest_call_pending && state.guest_call_undo.is_none());
}
