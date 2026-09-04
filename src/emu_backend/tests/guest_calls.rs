use super::*;

#[test]
fn failed_guest_call_restores_the_full_state() {
    let mut backend = build_gb_backend();
    backend.debug_suspend();
    let before = backend.encode_state_bytes().unwrap();
    let error = backend
        .execute_guest_call(&GuestCallRequest {
            name: "NeverReturns".to_owned(),
            target: 0x0150,
            storage_offset: None,
            explicit_overlay: false,
            exec_mode: ExecMode::Sm83,
            instruction_budget: 3,
        })
        .unwrap_err();

    assert!(error.to_string().contains("state restored"));
    assert_eq!(backend.encode_state_bytes().unwrap(), before);
}

#[test]
fn guest_call_rejects_a_stale_rom_mapping() {
    let mut backend = build_gb_backend();
    backend.debug_suspend();
    let error = backend
        .execute_guest_call(&GuestCallRequest {
            name: "WrongBank".to_owned(),
            target: 0x0150,
            storage_offset: Some(0x4150),
            explicit_overlay: false,
            exec_mode: ExecMode::Sm83,
            instruction_budget: 3,
        })
        .unwrap_err();

    assert!(error.to_string().contains("no longer maps"));
}

#[test]
fn pce_guest_call_returns_to_suspended_context_and_produces_undo_state() {
    let mut rom = build_pce_test_rom();
    rom[6..13].copy_from_slice(&[0xBA, 0xE8, 0xE8, 0x9A, 0x4C, 0x00, 0xE0]);
    let mut backend = load_test_backend_with_shared_loader(ActiveSystem::Pce, "call.pce", rom);
    backend.debug_suspend();

    let (instructions, undo_state) = backend
        .execute_guest_call(&GuestCallRequest {
            name: "GuestRoutine".to_owned(),
            target: 0xE006,
            storage_offset: Some(6),
            explicit_overlay: false,
            exec_mode: ExecMode::HuC6280,
            instruction_budget: 10,
        })
        .unwrap();

    assert_eq!(instructions, 5);
    assert!(!undo_state.is_empty());
    assert!(backend.is_suspended());
    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert_eq!(pce.debug_cpu_snapshot().registers().pc, 0xE000);
}

#[test]
fn coleco_guest_call_returns_to_suspended_context_and_produces_undo_state() {
    let mut rom = vec![0; 8 * 1024];
    rom[..2].copy_from_slice(&[0xAA, 0x55]);
    let mut bios = vec![0; zeff_coleco_core::constants::BIOS_SIZE];
    bios[..4].copy_from_slice(&[0x31, 0x00, 0x70, 0x00]);
    bios[0x100..0x103].copy_from_slice(&[0x3E, 0x42, 0xC9]);
    let mut emu = zeff_coleco_core::Emulator::new(&rom, &bios, 44_100).unwrap();
    emu.step_instruction();
    let mut backend = EmuBackend::from_coleco(
        emu,
        PathBuf::from("call.col"),
        crate::emu_backend::ColecoBackend::rom_hash_for_bytes(&rom),
    );
    backend.debug_suspend();

    let (instructions, undo_state) = backend
        .execute_guest_call(&GuestCallRequest {
            name: "GuestRoutine".to_owned(),
            target: 0x0100,
            storage_offset: None,
            explicit_overlay: false,
            exec_mode: ExecMode::Z80,
            instruction_budget: 10,
        })
        .unwrap();

    assert_eq!(instructions, 2);
    assert!(!undo_state.is_empty());
    assert!(backend.is_suspended());
    let coleco = backend.coleco().unwrap();
    assert_eq!(coleco.emu.cpu().regs().pc, 3);
    assert_eq!(coleco.emu.cpu().regs().a, 0x42);
}

#[test]
fn failed_pce_guest_call_restores_the_full_state() {
    let mut backend = build_pce_backend();
    backend.debug_suspend();
    let before = backend.encode_state_bytes().unwrap();
    let error = backend
        .execute_guest_call(&GuestCallRequest {
            name: "NeverReturns".to_owned(),
            target: 0xE001,
            storage_offset: Some(1),
            explicit_overlay: false,
            exec_mode: ExecMode::HuC6280,
            instruction_budget: 3,
        })
        .unwrap_err();

    assert!(error.to_string().contains("state restored"));
    assert_eq!(backend.encode_state_bytes().unwrap(), before);
}
