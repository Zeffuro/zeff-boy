use super::*;

#[test]
fn debuggable_adapter_exposes_uniform_cpu_peek_write() {
    let mut gb = zeff_gb_core::emulator::Emulator::from_rom_data(
        &build_gb_test_rom(),
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
    )
    .expect("Game Boy emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut gb, 0xC000, 0x12);

    let mut gba = zeff_gba_core::emulator::Emulator::new(&build_gba_test_rom(), 44_100)
        .expect("GBA emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut gba, 0x0200_0000, 0x34);

    let mut nes = zeff_nes_core::emulator::Emulator::new(&build_nes_test_rom(), 44_100.0)
        .expect("NES emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut nes, 0x0000, 0x56);

    let mut ws = zeff_ws_core::emulator::Emulator::new(&build_ws_test_rom(), 44_100)
        .expect("WonderSwan emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut ws, 0x0000_1234, 0x78);

    let mut sega8 = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &build_sms_test_rom(),
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .expect("Sega 8-bit emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut sega8, 0xC123, 0x9A);

    let mut coleco_rom = [0; 8 * 1024];
    coleco_rom[..2].copy_from_slice(&[0xAA, 0x55]);
    let mut coleco = zeff_coleco_core::Emulator::new(
        &coleco_rom,
        &[0; zeff_coleco_core::constants::BIOS_SIZE],
        44_100,
    )
    .expect("ColecoVision emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut coleco, 0x6123, 0xBC);
}

#[test]
fn coleco_execution_controls_and_trace_route_through_backend_runtime() {
    let mut backend = build_coleco_backend();
    assert!(backend.coleco().is_some());
    assert!(backend.supports_debugger());
    assert!(backend.supports_execution_controls());
    assert!(backend.supports_opcode_history());
    assert!(backend.supports_guest_calls());

    let mut actions = DebugUiActions::none();
    actions.add_breakpoint = Some(0);
    actions.trace_enabled = Some(true);
    let mut config = BackendRuntimeConfig::new(&actions);
    config.opcode_log_enabled = true;
    backend.apply_runtime_config(config);
    backend.step_frame();
    assert!(backend.is_suspended());

    let actions = DebugUiActions::none();
    let mut config = BackendRuntimeConfig::new(&actions);
    config.opcode_log_enabled = true;
    config.debug_step = true;
    backend.apply_runtime_config(config);

    let EmuBackend::Coleco(coleco) = &backend else {
        panic!("ColecoVision backend changed systems");
    };
    assert_eq!(coleco.emu.cpu().regs().pc, 1);
    assert_eq!(coleco.emu.recent_opcodes(1), vec![(0, 0, 4)]);
    assert_eq!(coleco.emu.instruction_trace().iter().count(), 1);
    assert!(coleco.emu.is_suspended());

    let mut config = BackendRuntimeConfig::new(&actions);
    config.debug_continue = true;
    backend.apply_runtime_config(config);
    assert!(!backend.is_suspended());
}

#[test]
fn coleco_backend_applies_bounded_raw_ram_cheats() {
    let mut backend = build_coleco_backend();
    let capabilities = backend.capabilities();
    assert!(capabilities.supports_cheats);
    assert!(capabilities.cheat_features.supports_ram_writes);
    assert!(!capabilities.cheat_features.supports_rom_patches);
    assert_eq!(capabilities.cheat_features.formats, ["Raw"]);

    backend.apply_ram_cheats(&[CheatPatch::RamWrite {
        address: 0x6123,
        value: CheatValue::Constant(0xA5),
    }]);

    assert_eq!(backend.coleco().unwrap().emu.cpu_peek8(0x6123), 0xA5);
}

#[test]
fn pce_execution_controls_step_and_record_history_through_the_backend_runtime() {
    let mut backend = build_pce_backend();
    assert!(backend.supports_debugger());
    assert!(backend.supports_execution_controls());
    assert!(backend.supports_opcode_history());

    backend.debug_suspend();
    assert!(backend.is_suspended());

    let actions = DebugUiActions::none();
    let mut config = BackendRuntimeConfig::new(&actions);
    config.opcode_log_enabled = true;
    config.debug_step = true;
    backend.apply_runtime_config(config);
    backend.step_frame();

    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert!(pce.is_cpu_suspended());
    assert_eq!(pce.debug_cpu_snapshot().registers().pc, 0xE001);
    let history = pce.recent_opcodes(1);
    assert_eq!(history[0].logical_pc(), 0xE000);
    assert_eq!(history[0].opcode(), 0xD4);

    let mut config = BackendRuntimeConfig::new(&actions);
    config.debug_continue = true;
    backend.apply_runtime_config(config);
    assert!(!backend.is_suspended());
}

#[test]
fn pce_runtime_routes_logical_breakpoint_and_watchpoint_actions() {
    let mut backend = build_pce_backend();
    let mut actions = DebugUiActions::none();
    actions.add_breakpoint = Some(0xE000);
    actions.add_watchpoint = Some((0x4000, 0x400F, WatchType::ReadWrite));
    actions
        .event_breakpoint_changes
        .push((DebugEvent::Interrupt, true));

    backend.apply_runtime_config(BackendRuntimeConfig::new(&actions));
    backend.step_frame();

    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert!(pce.is_cpu_suspended());
    assert_eq!(pce.debug_hit_breakpoint(), Some(0xE000));
    assert_eq!(pce.debug_watchpoints().len(), 1);
    assert_eq!(pce.debug_watchpoints()[0].address, 0x4000);
    assert_eq!(
        pce.iter_event_breakpoints().collect::<Vec<_>>(),
        [DebugEvent::Interrupt]
    );
}

#[test]
fn pce_runtime_routes_trace_configuration_and_dma_events() {
    let mut backend = build_pce_backend();
    backend.debug_suspend();
    let mut actions = DebugUiActions::none();
    actions.trace_enabled = Some(true);
    actions.trace_capacity = Some(3_000);
    actions
        .event_breakpoint_changes
        .extend([(DebugEvent::Interrupt, true), (DebugEvent::Dma, true)]);
    let mut config = BackendRuntimeConfig::new(&actions);
    config.debug_step = true;

    backend.apply_runtime_config(config);
    backend.step_frame();

    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert_eq!(
        pce.iter_event_breakpoints().collect::<Vec<_>>(),
        [DebugEvent::Interrupt, DebugEvent::Dma]
    );
    assert_eq!(pce.instruction_trace().capacity(), 3_000);
    let entries = pce.instruction_trace().iter().collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].mode, TraceExecMode::HuC6280);
    assert_eq!(entries[0].pc, 0xE000);
    assert_eq!(entries[0].instruction_bytes(), &[0xD4]);
}

#[test]
fn pce_runtime_only_collects_waveforms_while_apu_capture_is_requested() {
    let mut backend = build_pce_backend();
    let actions = DebugUiActions::none();
    let mut config = BackendRuntimeConfig::new(&actions);
    config.apu_capture_enabled = true;
    config.skip_audio = true;
    backend.apply_runtime_config(config);
    backend.step_frame();

    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert!(pce.debug_hardware_snapshot().psg.debug_capture_enabled);
    let retained = pce.psg_master_debug_samples_ordered().len();
    assert!(retained > 0);
    assert_eq!(pce.psg_channel_debug_samples_ordered(5).len(), retained);

    backend.apply_runtime_config(BackendRuntimeConfig::new(&actions));
    backend.step_frame();
    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert!(!pce.debug_hardware_snapshot().psg.debug_capture_enabled);
    assert_eq!(pce.psg_master_debug_samples_ordered().len(), retained);
}

#[test]
fn nes_runtime_skip_audio_controls_pcm_generation() {
    let mut backend = build_nes_backend();
    let actions = DebugUiActions::none();
    let mut muted = BackendRuntimeConfig::new(&actions);
    muted.apu_capture_enabled = true;
    muted.skip_audio = true;
    backend.apply_runtime_config(muted);
    backend.step_frame();

    let mut samples = Vec::new();
    backend.drain_audio_samples_into(&mut samples);
    assert!(samples.is_empty());

    backend.apply_runtime_config(BackendRuntimeConfig::new(&actions));
    backend.step_frame();
    backend.drain_audio_samples_into(&mut samples);
    assert!(!samples.is_empty());
}

fn assert_debuggable_cpu_byte_access(
    emu: &mut impl DebuggableEmulator,
    address: zeff_emu_common::address::Address,
    value: u8,
) {
    emu.cpu_write8(address, value);
    assert_eq!(emu.cpu_peek8(address), value);
}

#[test]
fn ws_backend_debug_actions_update_core_debug_state() {
    let rom = build_ws_test_rom();
    let emu = zeff_ws_core::emulator::Emulator::new(&rom, 44_100)
        .expect("WonderSwan emulator should initialize");
    let mut backend = EmuBackend::from_ws(emu, PathBuf::from("test.ws"));
    let mut actions = DebugUiActions::none();
    actions.add_breakpoint = Some(0xF0000);
    actions.add_one_shot_breakpoint = Some(0xF0010);
    actions.add_breakpoint_after = Some((0xF0020, 4));
    actions
        .event_breakpoint_changes
        .push((DebugEvent::Interrupt, true));
    actions.add_watchpoint = Some((0x0000, 0x000F, WatchType::Write));
    actions.memory_writes.push((0x0000, 0x5A));

    backend.apply_runtime_config(BackendRuntimeConfig::new(&actions));

    let ws = backend
        .ws()
        .expect("backend should remain WonderSwan after debug actions");
    assert_eq!(
        ws.emu.iter_breakpoints().collect::<Vec<_>>(),
        vec![0xF0000, 0xF0010, 0xF0020]
    );
    assert_eq!(
        ws.emu.iter_one_shot_breakpoints().collect::<Vec<_>>(),
        vec![0xF0010]
    );
    assert_eq!(
        ws.emu.iter_breakpoint_hit_conditions().collect::<Vec<_>>()[0].target_hits,
        4
    );
    assert_eq!(
        ws.emu.iter_event_breakpoints().collect::<Vec<_>>(),
        vec![DebugEvent::Interrupt]
    );
    assert_eq!(ws.emu.debug_watchpoints().len(), 1);
    assert_eq!(ws.emu.debug_watchpoints()[0].end_address, 0x000F);
    assert_eq!(
        ws.emu
            .debug_hit_watchpoint()
            .map(|hit| (hit.address, hit.new_value)),
        Some((0x0000, 0x5A))
    );

    let mut actions = DebugUiActions::none();
    actions
        .remove_watchpoints
        .push((0x0000, 0x000F, WatchType::Write));
    backend.apply_runtime_config(BackendRuntimeConfig::new(&actions));
    assert!(
        backend
            .ws()
            .expect("backend should remain WonderSwan")
            .emu
            .debug_watchpoints()
            .is_empty()
    );
}
