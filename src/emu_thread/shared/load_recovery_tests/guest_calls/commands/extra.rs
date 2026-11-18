use super::*;
use zeff_emu_common::save_state::StateReader;
use zeff_m68k::timing::{InterruptState, PrefetchCpu};

fn irq_state(backend: &EmuBackend) -> InterruptState {
    let raw = backend.encode_state_bytes().unwrap();
    let mut reader = StateReader::new(&raw);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"ZMDRAW01");
    assert_eq!(reader.read_u32().unwrap(), 1);
    let mut digest = [0; 32];
    reader.read_exact(&mut digest).unwrap();
    reader.read_u32().unwrap();
    reader.read_u8().unwrap();
    reader.read_bool().unwrap();
    reader.read_u8().unwrap();
    reader.read_u8().unwrap();
    let mut cpu = PrefetchCpu::default();
    cpu.read_boundary_state(&mut StateReader::new(reader.read_slice(128).unwrap()))
        .unwrap();
    *cpu.interrupt_input()
}

fn completed(owner: &mut Owner, target: u32, budget: u64) -> (u64, Vec<u8>) {
    let effects = owner.dispatch(EmuCommand::ExecuteGuestCall(request(target, budget)));
    assert!(effects.potentially_dirty);
    let Some(EmuResponse::GuestCallCompleted {
        instructions,
        undo_state,
        ..
    }) = effects.response
    else {
        panic!("guest call failed");
    };
    (instructions, undo_state)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_guest_call_final_rts_stack_watch_rolls_back_and_preserves_previous_undo() {
    let mut owner = Owner::new(standard());
    let original = frontend(&owner.backend);
    let original_host = session(&mut owner.backend);
    let token = owner.call();
    let stack = machine(&owner.backend)
        .registers()
        .stack_pointer()
        .wrapping_sub(4)
        & 0xff_ffff;
    let mut actions = DebugUiActions::none();
    actions.add_watchpoint = Some((stack, stack + 3, WatchType::Read));
    configure(&mut owner.backend, &actions);
    let before = frontend(&owner.backend);
    let host = session(&mut owner.backend);
    let markers = owner.markers.clone();
    let effects = owner.dispatch(EmuCommand::ExecuteGuestCall(request(0x1000, 8)));
    assert!(!effects.potentially_dirty);
    let Some(EmuResponse::GuestCallFailed { error, .. }) = effects.response else {
        panic!("final RTS watch was ignored");
    };
    assert!(error.contains("debugger stop") && error.contains("state restored"));
    assert_eq!(frontend(&owner.backend), before);
    assert_eq!(session(&mut owner.backend), host);
    assert_eq!(owner.markers, markers);
    assert!(owner.backend.take_runtime_fault().is_none());
    owner.undo(token);
    assert_eq!(frontend(&owner.backend), original);
    assert_eq!(session(&mut owner.backend), original_host);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_guest_call_real_rts_retains_callee_written_return_prefetch() {
    let main = [
        0x23fc, 0x60fe, 0x4e71, 0x00ff, 0x2000, 0x4ef9, 0x00ff, 0x2000,
    ];
    let callee = [
        0x33fc, 0x7005, 0x00ff, 0x2000, 0x33fc, 0x7206, 0x00ff, 0x2002, 0x70ff, 0x4e75, 0x4e71,
    ];
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = Owner::new(fixture(
            b'U',
            timing,
            ControllerKind::ThreeButton,
            &[(0x21c, &main), (0x1700, &callee)],
        ));
        assert_eq!(machine(&owner.backend).registers().pc, 0xff2000);
        assert_eq!(
            machine(&owner.backend).queued_words(),
            Some([0x60fe, 0x4e71])
        );
        let before = frontend(&owner.backend);
        let host = session(&mut owner.backend);
        let (instructions, token) = completed(&mut owner, 0x1700, 4);
        assert_eq!(instructions, 4);
        let actual = machine(&owner.backend);
        assert_eq!(actual.registers().pc, 0xff2000);
        assert_eq!(actual.registers().d[0], u32::MAX);
        assert_eq!(actual.registers().status_register(), 0x2318);
        assert_eq!(actual.queued_words(), Some([0x7005, 0x7206]));
        assert_eq!(
            (actual.peek(0xff2000), actual.peek(0xff2002)),
            (Some(0x70), Some(0x72))
        );
        owner.undo(token);
        assert_eq!(frontend(&owner.backend), before);
        assert_eq!(session(&mut owner.backend), host);
        assert_eq!(
            machine(&owner.backend).queued_words(),
            Some([0x60fe, 0x4e71])
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_guest_call_callee_trace_handlers_count_real_instructions_and_budget_atomically() {
    let callee = [0x46fc, 0xa715, 0x70ff, 0x4e75, 0x4e71];
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = Owner::new(fixture(
            b'U',
            timing,
            ControllerKind::ThreeButton,
            &[
                (0x24, &[0, 0x1800]),
                (0x1700, &callee),
                (0x1800, &[0x4e73, 0x4e71]),
            ],
        ));
        let original = frontend(&owner.backend);
        let original_host = session(&mut owner.backend);
        let saved_irq = irq_state(&owner.backend);
        let (instructions, token) = completed(&mut owner, 0x1700, 5);
        assert_eq!(instructions, 5);
        assert_eq!(
            machine(&owner.backend).registers().status_register(),
            0x2318
        );
        assert_eq!(irq_state(&owner.backend), saved_irq);
        let trace = session(&mut owner.backend).trace;
        assert_eq!(
            trace.iter().map(|row| row.pc).collect::<Vec<_>>(),
            [0x1700, 0x1704, 0x1800, 0x1706, 0x1800]
        );
        assert_eq!(
            trace
                .iter()
                .map(|row| row.boundary.unwrap().final_exception)
                .collect::<Vec<_>>(),
            [None, Some(9), None, Some(9), None]
        );
        assert!(
            trace
                .iter()
                .all(|row| row.boundary.unwrap().kind == TraceBoundaryKind::Instruction)
        );
        let before = frontend(&owner.backend);
        let host = session(&mut owner.backend);
        let markers = owner.markers.clone();
        let effects = owner.dispatch(EmuCommand::ExecuteGuestCall(request(0x1700, 4)));
        assert!(!effects.potentially_dirty);
        let Some(EmuResponse::GuestCallFailed { error, .. }) = effects.response else {
            panic!("trace handler instruction was omitted from the budget");
        };
        assert!(error.contains("budget"));
        assert_eq!(frontend(&owner.backend), before);
        assert_eq!(session(&mut owner.backend), host);
        assert_eq!(owner.markers, markers);
        owner.undo(token);
        assert_eq!(frontend(&owner.backend), original);
        assert_eq!(session(&mut owner.backend), original_host);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_guest_call_real_irq_entry_is_not_an_instruction_and_restores_the_irq_capsule() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = Owner::new(fixture(
            b'U',
            timing,
            ControllerKind::ThreeButton,
            &[
                (0x70, &[0, 0x1800]),
                (0x1700, &[0x46fc, 0x2000, 0x4e75, 0x4e71]),
                (0x1800, &[0x0057, 0x0700, 0x4e73, 0x4e71]),
            ],
        ));
        assert!(machine_mut(&mut owner.backend).set_interrupt_level(4));
        let before = frontend(&owner.backend);
        let host = session(&mut owner.backend);
        let saved_irq = irq_state(&owner.backend);
        assert_eq!(saved_irq.level(), 0);
        let (instructions, token) = completed(&mut owner, 0x1700, 4);
        assert_eq!(instructions, 4);
        assert_eq!(
            machine(&owner.backend).registers().status_register(),
            0x2300
        );
        assert_eq!(irq_state(&owner.backend), saved_irq);
        assert_eq!(machine(&owner.backend).interrupt_level(), 4);
        let trace = session(&mut owner.backend).trace;
        let kinds: Vec<_> = trace.iter().map(|row| row.boundary.unwrap().kind).collect();
        assert_eq!(
            kinds,
            [
                TraceBoundaryKind::Instruction,
                TraceBoundaryKind::Interrupt,
                TraceBoundaryKind::Instruction,
                TraceBoundaryKind::Instruction,
                TraceBoundaryKind::Instruction
            ]
        );
        let actions = DebugUiActions::none();
        let mut config = BackendRuntimeConfig::new(&actions);
        config.debug_step = true;
        config.opcode_log_enabled = true;
        owner.backend.apply_runtime_config(config);
        assert_eq!(machine(&owner.backend).registers().pc, 0x1800);
        let trace = session(&mut owner.backend).trace;
        assert_eq!(
            trace.last().unwrap().boundary.unwrap().kind,
            TraceBoundaryKind::Interrupt
        );
        owner.undo(token);
        assert_eq!(frontend(&owner.backend), before);
        assert_eq!(session(&mut owner.backend), host);
    }
}
