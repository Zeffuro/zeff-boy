use super::*;

const RESET_PC: u16 = 0xE000;

fn rom_with_program(program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[..program.len()].copy_from_slice(program);
    rom[0x1FFE..0x2000].copy_from_slice(&RESET_PC.to_le_bytes());
    rom
}

#[test]
fn physical_cheat_ram_follows_base_mirroring_and_supergrafx_banks() {
    let mut base_machine = PceMachine::with_cartridge_and_controller(
        rom_with_program(&[0xEA]),
        PceCartridgeDescriptor::default(),
        ControllerPort::two_button(),
    )
    .unwrap();
    zeff_pce_core::hardware::apply_pce_cheats(
        &mut base_machine,
        &[zeff_emu_common::cheats::CheatPatch::WideRamWrite {
            address: 0x1F_2345,
            value: zeff_emu_common::cheats::CheatValue::Constant(0x42),
        }],
    );
    assert_eq!(base_machine.mapped_work_ram()[0x345], 0x42);

    let mut supergrafx_target = synthetic_supergrafx_backend();
    zeff_pce_core::hardware::apply_pce_cheats(
        &mut supergrafx_target.machine,
        &[zeff_emu_common::cheats::CheatPatch::WideRamWrite {
            address: 0x1F_2345,
            value: zeff_emu_common::cheats::CheatValue::Constant(0x66),
        }],
    );
    assert_eq!(supergrafx_target.machine.mapped_work_ram()[0x2345], 0x66);
    assert_eq!(supergrafx_target.machine.mapped_work_ram()[0x0345], 0x00);
}

fn memory_base_clock(controller: &mut ControllerPort, bit: bool) {
    controller.write_lines(bit, false);
    controller.write_lines(bit, true);
}

fn memory_base_send(controller: &mut ControllerPort, value: u32, bit_count: u8) {
    for bit in 0..bit_count {
        memory_base_clock(controller, value & (1 << bit) != 0);
    }
}

fn memory_base_write_byte(backend: &mut PceBackend, value: u8) {
    let controller = backend.machine.devices_mut().controller_mut();
    memory_base_send(controller, 0xA8, 8);
    memory_base_clock(controller, false);
    memory_base_clock(controller, false);
    memory_base_clock(controller, false);
    memory_base_send(controller, 0, 10);
    memory_base_send(controller, 8, 20);
    memory_base_send(controller, u32::from(value), 8);
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("zeff-pce-{label}-{}-{nonce}", std::process::id()))
}

fn backend_with_board(board: PceHuCardBoard, image_len: usize) -> PceBackend {
    let mut rom = rom_with_program(&[0xA9, 0x5A, 0x8D, 0x00, 0x40]);
    rom.resize(image_len, 0xEA);
    let descriptor = PceCartridgeDescriptor::default().with_hucard_board(board);
    let machine =
        PceMachine::with_cartridge_and_controller(rom, descriptor, ControllerPort::two_button())
            .unwrap();
    let mut backend = PceBackend {
        machine,
        paths: BackendPaths::new(PathBuf::from("board.pce")),
        rom_hash: [0; 32],
        source_crc32: None,
        source_disc_hash: None,
        framebuffer: vec![0; PCE_PRESENTED_RGBA_BYTES].into_boxed_slice(),
        frame_count: 0,
        pending_runtime_fault: None,
        overscan_mode: PceOverscanMode::default(),
        palette_mode: PcePaletteMode::default(),
        pce_controller_mode: PceControllerMode::TwoButton,
        pce_memory_base_mode: PceMemoryBaseMode::Disabled,
        pce_arcade_card_mode: PceArcadeCardMode::Disabled,
        mouse_host_buttons: PadButtons::empty(),
    };
    backend.project_presented_frame();
    backend
}

#[test]
fn debuggable_adapter_exposes_logical_breakpoints_watchpoints_and_writes() {
    let mut backend =
        PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("debug.pce")).unwrap();
    backend
        .machine
        .cpu_mut()
        .cpu_mut()
        .set_mapping_register(2, 0xF8);

    DebuggableEmulator::add_breakpoint(&mut backend, Address::from(RESET_PC));
    DebuggableEmulator::add_watchpoint_range(&mut backend, 0x4000, 0x4000, WatchType::Write);
    DebuggableEmulator::cpu_write8(&mut backend, 0x4000, 0x5A);

    assert_eq!(DebuggableEmulator::cpu_peek8(&backend, 0x4000), 0x5A);
    assert_eq!(backend.iter_breakpoints().collect::<Vec<_>>(), [0xE000]);
    let hit = backend
        .debug_hit_watchpoint()
        .expect("debug write should hit logical watchpoint");
    assert_eq!(hit.address, 0x4000);
    assert_eq!(hit.old_value, 0);
    assert_eq!(hit.new_value, 0x5A);
    assert!(backend.is_cpu_suspended());

    DebuggableEmulator::add_breakpoint(&mut backend, 0x1_0000);
    assert_eq!(backend.iter_breakpoints().collect::<Vec<_>>(), [0xE000]);
}

#[test]
fn debuggable_adapter_routes_events_and_instruction_trace() {
    let mut backend =
        PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("trace.pce")).unwrap();
    DebuggableEmulator::set_event_breakpoint(&mut backend, DebugEvent::Interrupt, true);
    DebuggableEmulator::set_event_breakpoint(&mut backend, DebugEvent::Dma, true);
    DebuggableEmulator::set_instruction_trace_capacity(&mut backend, 2_500);
    DebuggableEmulator::set_instruction_trace_enabled(&mut backend, true);

    backend.debug_suspend();
    backend.debug_step();
    backend.step_frame();

    assert_eq!(
        backend.iter_event_breakpoints().collect::<Vec<_>>(),
        [DebugEvent::Interrupt, DebugEvent::Dma]
    );
    assert!(backend.instruction_trace().is_enabled());
    assert_eq!(backend.instruction_trace().capacity(), 2_500);
    let entry = backend.instruction_trace().iter().next().unwrap();
    assert_eq!(entry.mode, zeff_emu_common::debug::TraceExecMode::HuC6280);
    assert_eq!(entry.pc, u32::from(RESET_PC));
    assert_eq!(entry.bank, Some(0));
    assert_eq!(entry.instruction_bytes(), &[0xEA]);

    DebuggableEmulator::clear_instruction_trace(&mut backend);
    assert!(backend.instruction_trace().is_empty());
}

#[test]
fn structural_sf2_image_is_admitted_without_relaxing_plain_cards() {
    let mut rom = rom_with_program(&[0xEA]);
    rom.resize(zeff_pce_core::hardware::SF2_CE_HUCARD_IMAGE_LEN, 0xEA);
    let backend = PceBackend::new(rom, PathBuf::from("sf2.pce")).unwrap();
    assert_eq!(backend.hucard_board(), PceHuCardBoard::Sf2Ce);
    assert_eq!(backend.hucard_rom().len(), 0x28_0000);

    let oversized_plain = vec![0; 0x10_0000 + 0x2000];
    assert!(PceBackend::new(oversized_plain, PathBuf::from("unknown.pce")).is_err());
}

#[test]
fn exact_lemmings_disc_automatically_selects_mouse_but_force_pad_wins() {
    let mut backend = backend_with_board(PceHuCardBoard::SystemCardV3, 0x40_000);
    backend.rom_hash = [0; 32];
    backend.source_disc_hash = Some(LEMMINGS_JAPAN_CANONICAL_DISC_SHA256);

    backend.set_pce_mouse_state(PceControllerMode::Automatic, 1, 2, 1);
    assert!(matches!(
        backend.machine.devices().controller().device(),
        zeff_pce_core::hardware::ControllerDevice::Mouse(_)
    ));

    backend.set_pce_mouse_state(PceControllerMode::TwoButton, 0, 0, 0);
    assert!(matches!(
        backend.machine.devices().controller().device(),
        zeff_pce_core::hardware::ControllerDevice::TwoButton(_)
    ));
}

#[test]
fn exact_deden_disc_automatically_selects_multitap_with_independent_five_pads() {
    let mut backend = backend_with_board(PceHuCardBoard::SystemCardV3, 0x40_000);
    backend.rom_hash = [0; 32];
    backend.source_disc_hash = Some(TENGAI_MAKYOU_DEDEN_NO_KABUKI_DEN_CANONICAL_DISC_SHA256);

    backend.set_pce_mouse_state(PceControllerMode::Automatic, 0, 0, 0);
    backend.set_input(1, 0);
    backend.set_input_p2(2, 0);
    backend.set_input_p3(4, 0);
    backend.set_input_p4(8, 0);
    backend.set_input_p5(0x10, 8);

    let zeff_pce_core::hardware::ControllerDevice::Multitap(multitap) =
        backend.machine.devices().controller().device()
    else {
        panic!("Deden no Kabuki-den did not select the multitap");
    };
    let zeff_pce_core::hardware::MultitapDevice::TwoButton(p1) =
        multitap.port(zeff_pce_core::hardware::MultitapPort::One)
    else {
        panic!("multitap port 1 is not a two-button pad");
    };
    let zeff_pce_core::hardware::MultitapDevice::TwoButton(p2) =
        multitap.port(zeff_pce_core::hardware::MultitapPort::Two)
    else {
        panic!("multitap port 2 is not a two-button pad");
    };
    let zeff_pce_core::hardware::MultitapDevice::TwoButton(p3) =
        multitap.port(zeff_pce_core::hardware::MultitapPort::Three)
    else {
        panic!("multitap port 3 is not a two-button pad");
    };
    let zeff_pce_core::hardware::MultitapDevice::TwoButton(p4) =
        multitap.port(zeff_pce_core::hardware::MultitapPort::Four)
    else {
        panic!("multitap port 4 is not a two-button pad");
    };
    let zeff_pce_core::hardware::MultitapDevice::TwoButton(p5) =
        multitap.port(zeff_pce_core::hardware::MultitapPort::Five)
    else {
        panic!("multitap port 5 is not a two-button pad");
    };
    assert_eq!(p1.buttons(), PadButtons::I);
    assert_eq!(p2.buttons(), PadButtons::II);
    assert_eq!(p3.buttons(), PadButtons::SELECT);
    assert_eq!(p4.buttons(), PadButtons::RUN);
    assert_eq!(p5.buttons(), PadButtons::DOWN);
}

#[test]
fn manual_six_button_mode_maps_all_host_button_bits() {
    let mut backend = PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("six.pce")).unwrap();

    backend.set_pce_mouse_state(PceControllerMode::SixButton, 0, 0, 0);
    backend.set_input(0xFF, 0x0F);

    let zeff_pce_core::hardware::ControllerDevice::SixButton(pad) =
        backend.machine.devices().controller().device()
    else {
        panic!("manual six-button mode did not install a six-button pad");
    };
    assert_eq!(
        pad.standard_pad().buttons(),
        PadButtons::I
            | PadButtons::II
            | PadButtons::SELECT
            | PadButtons::RUN
            | PadButtons::UP
            | PadButtons::RIGHT
            | PadButtons::DOWN
            | PadButtons::LEFT
    );
    assert_eq!(
        pad.extra_buttons(),
        zeff_pce_core::hardware::SixButtonExtraButtons::III
            | zeff_pce_core::hardware::SixButtonExtraButtons::IV
            | zeff_pce_core::hardware::SixButtonExtraButtons::V
            | zeff_pce_core::hardware::SixButtonExtraButtons::VI
    );
    assert_eq!(backend.controller_mode(), PceControllerMode::SixButton);
    assert!(matches!(
        backend.debug_hardware_snapshot().controller.device,
        zeff_pce_core::hardware::ControllerDeviceDebugSnapshot::SixButton {
            extra_buttons,
            ..
        } if extra_buttons == zeff_pce_core::hardware::SixButtonExtraButtons::all()
    ));
}

#[test]
fn pceas_development_header_is_validated_and_removed_before_hashing() {
    let mut payload = rom_with_program(&[0xEA]);
    payload.resize(3 * HUCARD_BANK_LEN, 0xEA);
    let expected_hash = zeff_firmware::sha256_bytes(&payload);
    let mut headered = vec![0; PCEAS_HEADER_LEN];
    headered[0] = 3;
    headered.extend_from_slice(&payload);

    let backend = PceBackend::new(headered, PathBuf::from("source-test.pce")).unwrap();
    assert_eq!(backend.hucard_rom(), payload);
    assert_eq!(backend.rom_hash(), expected_hash);

    let mut invalid = vec![0; PCEAS_HEADER_LEN];
    invalid[0] = 2;
    invalid.extend_from_slice(&payload);
    assert!(PceBackend::new(invalid, PathBuf::from("invalid.pce")).is_err());
}

#[test]
fn populous_ram_is_nonbattery_copyable_mapper_ram_with_state_capabilities() {
    let mut backend = backend_with_board(
        PceHuCardBoard::Populous,
        zeff_pce_core::hardware::POPULOUS_HUCARD_IMAGE_LEN,
    );
    assert_eq!(
        backend.save_ram_kind(),
        SaveRamKind::mapper_ram_unknown(POPULOUS_HUCARD_RAM_LEN)
    );
    assert!(!backend.save_ram_kind().is_battery_backed());
    assert!(backend.supports_save_states());
    assert!(backend.supports_rewind());
    assert!(backend.supports_replay());
    assert!(backend.supports_guest_calls());
    assert_eq!(backend.flush_battery_sram().unwrap(), None);

    backend
        .machine
        .cpu_mut()
        .cpu_mut()
        .set_mapping_register(2, 0x40);
    backend.machine.step_boundary().unwrap();
    backend.machine.step_boundary().unwrap();
    let mut ram = Vec::new();
    assert_eq!(
        backend.copy_memory_region("save_ram", &mut ram).unwrap(),
        MemoryRegionDescriptor::save_ram(POPULOUS_HUCARD_RAM_LEN)
    );
    assert_eq!(ram.len(), POPULOUS_HUCARD_RAM_LEN);
    assert_eq!(ram[0], 0x5A);
}

#[test]
fn cd_backup_ram_is_formatted_battery_backed_and_copyable() {
    let mut system_card = vec![0xEA; zeff_pce_core::hardware::SYSTEM_CARD_V1_V2_IMAGE_LEN];
    system_card[0x1FFE..0x2000].copy_from_slice(&RESET_PC.to_le_bytes());
    let disc = CdDisc::new(vec![
        zeff_pce_core::hardware::CdTrack::from_index1_data(
            1,
            4,
            None,
            0,
            zeff_pce_core::hardware::CdTrackMode::Mode1_2048,
            vec![0; zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
        )
        .unwrap(),
    ])
    .unwrap();
    let mut backend = PceBackend::new_cdrom2(
        system_card,
        disc,
        PceCdBackendConfig {
            system_card_board: PceHuCardBoard::SystemCardV1V2,
            cue_path: PathBuf::from("disc.cue"),
            source_path: PathBuf::from("disc.cue"),
            content_hash: [0; 32],
            content_crc32: 0,
            source_disc_hash: [0; 32],
            console_wiring: PceConsoleWiring::PcEngine,
            arcade_card_mode: PceArcadeCardMode::Disabled,
        },
    )
    .unwrap();

    assert_eq!(
        backend.save_ram_kind(),
        SaveRamKind::known_battery_backed(CDROM2_BRAM_LEN)
    );
    let mut ram = Vec::new();
    backend.copy_memory_region("save_ram", &mut ram).unwrap();
    assert_eq!(&ram[..8], b"HUBM\x00\xA0\x10\x80");

    let replacement = vec![0x5A; CDROM2_BRAM_LEN];
    backend.load_cd_bram(&replacement).unwrap();
    backend.copy_memory_region("save_ram", &mut ram).unwrap();
    assert_eq!(ram, replacement);
    assert!(
        backend
            .load_cd_bram(&replacement[..CDROM2_BRAM_LEN - 1])
            .is_err()
    );
}

#[test]
fn memory_base_manual_mode_exposes_copyable_battery_ram() {
    let mut backend =
        PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("mb128.pce")).unwrap();
    assert_eq!(backend.memory_base_mode(), PceMemoryBaseMode::Disabled);
    assert!(
        backend
            .memory_regions()
            .iter()
            .all(|region| region.id != "memory_base_128")
    );

    backend.update_memory_base_mode(PceMemoryBaseMode::Enabled);
    assert_eq!(backend.memory_base_mode(), PceMemoryBaseMode::Enabled);
    assert_eq!(
        backend.save_ram_kind(),
        SaveRamKind::known_battery_backed(MEMORY_BASE128_RAM_LEN)
    );
    let mut image = vec![0; MEMORY_BASE128_RAM_LEN];
    image[0x1234] = 0xA5;
    backend.load_memory_base128(&image).unwrap();
    let mut copied = Vec::new();
    let region = backend.copy_memory_region("mb128", &mut copied).unwrap();
    assert_eq!(region, MEMORY_BASE128_REGION);
    assert_eq!(copied, image);

    backend.update_memory_base_mode(PceMemoryBaseMode::Disabled);
    assert_eq!(backend.memory_base_mode(), PceMemoryBaseMode::Disabled);
    assert_eq!(backend.save_ram_kind(), SaveRamKind::none());
    assert_eq!(
        backend
            .machine
            .devices()
            .controller()
            .memory_base128()
            .ram()[0x1234],
        0xA5
    );
}

#[test]
fn memory_base_persistence_is_exact_transactional_and_coexists_with_cd_bram() {
    let temp_dir = unique_temp_dir("memory-base-persistence");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut backend = synthetic_cd_backend(PceHuCardBoard::SystemCardV1V2);
    backend.paths = BackendPaths::new(temp_dir.join("disc.cue"));
    let bram = vec![0x3C; CDROM2_BRAM_LEN];
    backend.load_cd_bram(&bram).unwrap();
    backend.update_memory_base_mode(PceMemoryBaseMode::Enabled);
    memory_base_write_byte(&mut backend, 0xA5);
    let memory_base_path = temp_dir.join("mb128.sav");

    let flushed = backend
        .flush_persistent_data(&memory_base_path)
        .unwrap()
        .unwrap();
    let bram_path = temp_dir.join("disc.sav");
    assert!(flushed.contains(&bram_path.display().to_string()));
    assert!(flushed.contains(&memory_base_path.display().to_string()));
    assert_eq!(std::fs::read(&bram_path).unwrap(), bram);
    let persisted = std::fs::read(&memory_base_path).unwrap();
    assert_eq!(persisted.len(), MEMORY_BASE128_RAM_LEN);
    assert_eq!(persisted[0], 0xA5);
    assert!(
        !backend
            .machine
            .devices()
            .controller()
            .memory_base128()
            .is_dirty()
    );

    let replacement = vec![0xCC; MEMORY_BASE128_RAM_LEN];
    backend.load_memory_base128(&replacement).unwrap();
    std::fs::write(&memory_base_path, &persisted[..MEMORY_BASE128_RAM_LEN - 1]).unwrap();
    assert!(
        backend
            .try_load_memory_base128_from_path(&memory_base_path)
            .is_err()
    );
    assert_eq!(
        backend
            .machine
            .devices()
            .controller()
            .memory_base128()
            .ram(),
        replacement
    );

    std::fs::remove_dir_all(temp_dir).unwrap();
}

fn synthetic_supergrafx_backend() -> PceBackend {
    let descriptor = PceCartridgeDescriptor::default()
        .with_required_hardware(zeff_pce_core::hardware::PceCartridgeHardware::SuperGrafx);
    let machine = PceMachine::with_cartridge_and_controller(
        rom_with_program(&[0xD4, 0xEA, 0x80, 0xFD]),
        descriptor,
        ControllerPort::two_button(),
    )
    .unwrap();
    let mut backend = PceBackend {
        machine,
        paths: BackendPaths::new(PathBuf::from("synthetic-sgx.pce")),
        rom_hash: [0x53; 32],
        source_crc32: None,
        source_disc_hash: None,
        framebuffer: vec![0; PCE_PRESENTED_RGBA_BYTES].into_boxed_slice(),
        frame_count: 0,
        pending_runtime_fault: None,
        overscan_mode: PceOverscanMode::default(),
        palette_mode: PcePaletteMode::default(),
        pce_controller_mode: PceControllerMode::TwoButton,
        pce_memory_base_mode: PceMemoryBaseMode::Disabled,
        pce_arcade_card_mode: PceArcadeCardMode::Disabled,
        mouse_host_buttons: PadButtons::empty(),
    };
    backend.project_presented_frame();
    backend
}

fn synthetic_cd_backend(board: PceHuCardBoard) -> PceBackend {
    synthetic_cd_backend_with_arcade_card(board, PceArcadeCardMode::Disabled).unwrap()
}

fn synthetic_cd_backend_with_arcade_card(
    board: PceHuCardBoard,
    arcade_card_mode: PceArcadeCardMode,
) -> anyhow::Result<PceBackend> {
    let mut system_card = vec![0xEA; zeff_pce_core::hardware::SYSTEM_CARD_V1_V2_IMAGE_LEN];
    system_card[0x1FFE..0x2000].copy_from_slice(&RESET_PC.to_le_bytes());
    let mut sectors = vec![0; 2 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    for (index, byte) in sectors.iter_mut().enumerate() {
        *byte = index as u8;
    }
    let disc = CdDisc::new(vec![
        zeff_pce_core::hardware::CdTrack::from_index1_data(
            1,
            4,
            None,
            0,
            zeff_pce_core::hardware::CdTrackMode::Mode1_2048,
            sectors,
        )
        .unwrap(),
    ])
    .unwrap();
    PceBackend::new_cdrom2(
        system_card,
        disc,
        PceCdBackendConfig {
            system_card_board: board,
            cue_path: PathBuf::from("synthetic.cue"),
            source_path: PathBuf::from("synthetic.cue"),
            content_hash: [board as u8; 32],
            content_crc32: board as u32,
            source_disc_hash: [board as u8; 32],
            console_wiring: PceConsoleWiring::PcEngine,
            arcade_card_mode,
        },
    )
}

fn assert_pce_backend_replay_is_deterministic(mut backend: PceBackend) {
    backend.set_apu_sample_generation_enabled(false);
    backend.step_frame_bounded().unwrap();
    let checkpoint_frame = backend.framebuffer().to_vec();
    let checkpoint = backend.encode_state_bytes().unwrap();

    backend.step_frame_bounded().unwrap();
    backend.step_frame_bounded().unwrap();
    let expected_frame = backend.framebuffer().to_vec();
    let expected = backend.encode_state_bytes().unwrap();

    backend.load_state_from_bytes(checkpoint).unwrap();
    assert_eq!(backend.framebuffer(), checkpoint_frame);
    backend.step_frame_bounded().unwrap();
    backend.step_frame_bounded().unwrap();
    assert_eq!(backend.framebuffer(), expected_frame);
    assert_eq!(backend.encode_state_bytes().unwrap(), expected);
}

#[test]
fn backend_state_replay_is_deterministic_for_every_supported_pce_topology() {
    assert_pce_backend_replay_is_deterministic(
        PceBackend::new(
            rom_with_program(&[0xD4, 0xEA, 0x80, 0xFD]),
            PathBuf::from("synthetic-base.pce"),
        )
        .unwrap(),
    );
    assert_pce_backend_replay_is_deterministic(synthetic_supergrafx_backend());
    assert_pce_backend_replay_is_deterministic(synthetic_cd_backend(
        PceHuCardBoard::SystemCardV1V2,
    ));
    assert_pce_backend_replay_is_deterministic(synthetic_cd_backend(PceHuCardBoard::SystemCardV3));
    let mut arcade = synthetic_cd_backend_with_arcade_card(
        PceHuCardBoard::SystemCardV3,
        PceArcadeCardMode::Enabled,
    )
    .unwrap();
    arcade
        .machine
        .devices_mut()
        .arcade_card_mut()
        .unwrap()
        .write_physical(0x1F_FA09, 0x13);
    assert_pce_backend_replay_is_deterministic(arcade);
}

#[test]
fn arcade_card_selection_rejects_incompatible_topology_and_exposes_volatile_ram() {
    let error = synthetic_cd_backend_with_arcade_card(
        PceHuCardBoard::SystemCardV1V2,
        PceArcadeCardMode::Enabled,
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("System Card v3"));

    let mut backend = synthetic_cd_backend_with_arcade_card(
        PceHuCardBoard::SystemCardV3,
        PceArcadeCardMode::Enabled,
    )
    .unwrap();
    assert_eq!(backend.arcade_card_mode(), PceArcadeCardMode::Enabled);
    assert!(backend.machine.devices().arcade_card().is_some());
    let region = backend
        .memory_regions()
        .into_iter()
        .find(|region| region.id == "arcade_card_ram")
        .unwrap();
    assert_eq!(region.kind, MemoryRegionKind::ExternalWorkRam);
    assert_eq!(region.size, Some(ARCADE_CARD_RAM_LEN));
    backend
        .machine
        .devices_mut()
        .arcade_card_mut()
        .unwrap()
        .ram_mut()[0x1F_FFFF] = 0xA5;
    let mut ram = Vec::new();
    backend.copy_memory_region("acram", &mut ram).unwrap();
    assert_eq!(ram.len(), ARCADE_CARD_RAM_LEN);
    assert_eq!(ram[0x1F_FFFF], 0xA5);
}

#[test]
fn backend_state_restores_owned_state_reprojects_and_clears_debug_history() {
    let mut backend = PceBackend::new(
        rom_with_program(&[0xD4, 0xEA, 0x80, 0xFD]),
        PathBuf::from("backend-state.pce"),
    )
    .unwrap();
    backend.set_pce_mouse_state(PceControllerMode::Mouse, 9, -7, 1);
    backend.set_opcode_history_enabled(true);
    backend.set_apu_debug_capture_enabled(true);
    DebuggableEmulator::set_instruction_trace_capacity(&mut backend, 64);
    DebuggableEmulator::set_instruction_trace_enabled(&mut backend, true);
    let trace_capacity = backend.instruction_trace().capacity();
    backend.step_frame_bounded().unwrap();
    assert!(!backend.recent_opcodes(1).is_empty());
    assert!(!backend.instruction_trace().is_empty());
    assert!(!backend.psg_master_debug_samples_ordered().is_empty());
    let saved_frame_count = backend.frame_count;
    let saved_framebuffer = backend.framebuffer().to_vec();
    let state = backend.encode_state_bytes().unwrap();

    backend.frame_count = 99;
    backend.framebuffer.fill(0x7F);
    backend.mouse_host_buttons = PadButtons::empty();
    backend.update_controller_mode(PceControllerMode::TwoButton);
    backend.pending_runtime_fault = Some("stale runtime fault".to_owned());
    backend.load_state_from_bytes(state).unwrap();

    assert_eq!(backend.frame_count, saved_frame_count);
    assert_eq!(backend.framebuffer(), saved_framebuffer);
    assert_eq!(backend.controller_mode(), PceControllerMode::Mouse);
    assert_eq!(backend.mouse_host_buttons, PadButtons::I);
    assert!(backend.pending_runtime_fault.is_none());
    assert!(backend.recent_opcodes(1).is_empty());
    assert!(backend.instruction_trace().is_enabled());
    assert_eq!(backend.instruction_trace().capacity(), trace_capacity);
    assert!(backend.instruction_trace().is_empty());
    assert!(backend.debug_hardware_snapshot().psg.debug_capture_enabled);
    assert!(backend.psg_master_debug_samples_ordered().is_empty());
    backend.machine.step_boundary().unwrap();
    assert!(!backend.recent_opcodes(1).is_empty());
    assert!(!backend.instruction_trace().is_empty());
}

#[test]
fn backend_state_rejection_is_transactional_and_fault_policy_is_explicit() {
    let saved =
        PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("saved-state.pce")).unwrap();
    let state = saved.encode_state_bytes().unwrap();

    let mut target_rom = rom_with_program(&[0xEA]);
    target_rom[0x100] ^= 0xFF;
    let mut target = PceBackend::new(target_rom, PathBuf::from("target.pce")).unwrap();
    target.step_frame_bounded().unwrap();
    let before = target.encode_state_bytes().unwrap();
    let before_frame = target.framebuffer().to_vec();
    target.pending_runtime_fault = Some("preserve on failed load".to_owned());
    assert!(target.load_state_from_bytes(state).is_err());
    assert_eq!(
        target.pending_runtime_fault.as_deref(),
        Some("preserve on failed load")
    );
    target.pending_runtime_fault = None;
    assert_eq!(target.encode_state_bytes().unwrap(), before);
    assert_eq!(target.framebuffer(), before_frame);

    let mut malformed = before.clone();
    malformed.pop();
    assert!(target.load_state_from_bytes(malformed).is_err());
    assert_eq!(target.encode_state_bytes().unwrap(), before);

    target.pending_runtime_fault = Some("pending".to_owned());
    assert!(
        target
            .encode_state_bytes()
            .unwrap_err()
            .to_string()
            .contains("faulted")
    );
}

#[test]
fn backend_state_roundtrips_connected_memory_base_ram_and_live_protocol() {
    let mut backend = PceBackend::new(
        rom_with_program(&[0xEA]),
        PathBuf::from("memory-base-state.pce"),
    )
    .unwrap();
    let mut ram = vec![0; zeff_pce_core::hardware::MEMORY_BASE128_RAM_LEN];
    for (index, byte) in ram.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(17);
    }
    let controller = backend.machine.devices_mut().controller_mut();
    controller.memory_base128_mut().load_ram(&ram).unwrap();
    controller.set_memory_base128_connected(true);
    for bit in [false, false, false, true, false, true, false, true] {
        controller.write_lines(bit, false);
        controller.write_lines(bit, true);
    }
    controller.write_lines(true, false);
    controller.write_lines(true, true);
    assert_eq!(
        controller.memory_base128().debug_snapshot().phase,
        zeff_pce_core::hardware::MemoryBase128Phase::IdentifySecond
    );
    let state = backend.encode_state_bytes().unwrap();

    let controller = backend.machine.devices_mut().controller_mut();
    controller.set_memory_base128_connected(false);
    controller
        .memory_base128_mut()
        .load_ram(&vec![0xFF; zeff_pce_core::hardware::MEMORY_BASE128_RAM_LEN])
        .unwrap();
    backend.load_state_from_bytes(state.clone()).unwrap();
    let restored = backend.machine.devices().controller().memory_base128();
    assert!(restored.is_connected());
    assert_eq!(backend.memory_base_mode(), PceMemoryBaseMode::Enabled);
    assert_eq!(restored.ram(), ram);
    assert_eq!(
        restored.debug_snapshot().phase,
        zeff_pce_core::hardware::MemoryBase128Phase::IdentifySecond
    );
    assert_eq!(backend.encode_state_bytes().unwrap(), state);
}

fn pixel(frame: &[u8], x: usize, y: usize) -> [u8; 4] {
    frame[(y * PCE_PRESENTED_WIDTH + x) * 4..][..4]
        .try_into()
        .unwrap()
}

#[test]
fn projection_maps_variable_active_rows_without_sampling_padding() {
    let mut source = vec![0; 4 * 2 * 4];
    let colors = [
        [1, 0, 0, 0xFF],
        [2, 0, 0, 0xFF],
        [3, 0, 0, 0xFF],
        [4, 0, 0, 0xFF],
        [5, 0, 0, 0xFF],
        [6, 0, 0, 0xFF],
        [0xEE, 0, 0, 0xFF],
        [0xEF, 0, 0, 0xFF],
    ];
    for (pixel, color) in source.as_chunks_mut::<4>().0.iter_mut().zip(colors) {
        pixel.copy_from_slice(&color);
    }
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 1,
            active: true,
        },
        ProjectionRow {
            active_x_origin: 0,
            active_width: 2,
            pixel_clock_divisor: 1,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_sgx_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);

    assert_eq!(pixel(&output, 0, 0), colors[0]);
    assert_eq!(pixel(&output, 639, 239), colors[3]);
    assert_eq!(pixel(&output, 159, 240), colors[4]);
    assert_eq!(pixel(&output, 160, 240), colors[5]);
    assert_eq!(pixel(&output, 319, 240), colors[5]);
    assert_eq!(pixel(&output, 320, 240), OPAQUE_BLACK);
    assert_eq!(pixel(&output, 639, 479), OPAQUE_BLACK);
}

#[test]
fn base_projection_scales_each_active_row_without_black_side_bars() {
    let mut source = vec![0; 4 * 2 * 4];
    let colors = [
        [1, 0, 0, 0xFF],
        [2, 0, 0, 0xFF],
        [3, 0, 0, 0xFF],
        [4, 0, 0, 0xFF],
        [5, 0, 0, 0xFF],
        [6, 0, 0, 0xFF],
        [0xEE, 0, 0, 0xFF],
        [0xEF, 0, 0, 0xFF],
    ];
    for (pixel, color) in source.as_chunks_mut::<4>().0.iter_mut().zip(colors) {
        pixel.copy_from_slice(&color);
    }
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 4,
            active: true,
        },
        ProjectionRow {
            active_x_origin: 7,
            active_width: 2,
            pixel_clock_divisor: 2,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_base_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);

    assert_eq!(pixel(&output, 0, 0), colors[0]);
    assert_eq!(pixel(&output, 639, 0), colors[3]);
    assert_eq!(pixel(&output, 0, 479), colors[4]);
    assert_eq!(pixel(&output, 319, 479), colors[4]);
    assert_eq!(pixel(&output, 320, 479), colors[5]);
    assert_eq!(pixel(&output, 639, 479), colors[5]);
}

#[test]
fn base_projection_preserves_the_complete_programmed_active_span() {
    const ACTIVE_WIDTH: usize = 352;
    const ACTIVE_HEIGHT: usize = 240;
    let mut source = vec![0; PCE_ACTIVE_FRAME_WIDTH * ACTIVE_HEIGHT * 4];
    for y in 0..ACTIVE_HEIGHT {
        for x in 0..ACTIVE_WIDTH {
            let offset = (y * PCE_ACTIVE_FRAME_WIDTH + x) * 4;
            source[offset..offset + 4].copy_from_slice(&[x as u8, y as u8, 0x40, 0xFF]);
        }
    }
    let rows = vec![
        ProjectionRow {
            active_x_origin: 0,
            active_width: ACTIVE_WIDTH,
            pixel_clock_divisor: 4,
            active: true,
        };
        ACTIVE_HEIGHT
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_base_rgba_rows(
        &source,
        PCE_ACTIVE_FRAME_WIDTH,
        &rows,
        Some((0, ACTIVE_HEIGHT)),
        &mut output,
    );

    assert_eq!(pixel(&output, 0, 0), [0, 0, 0x40, 0xFF]);
    assert_eq!(pixel(&output, 639, 479), [95, 239, 0x40, 0xFF]);
}

#[test]
fn projection_aligns_rows_in_one_master_dot_domain() {
    let mut source = vec![0; 4 * 2 * 4];
    let colors = [
        [1, 0, 0, 0xFF],
        [2, 0, 0, 0xFF],
        [0xA0, 0, 0, 0xFF],
        [4, 0, 0, 0xFF],
        [5, 0, 0, 0xFF],
        [6, 0, 0, 0xFF],
        [0xB0, 0, 0, 0xFF],
        [8, 0, 0, 0xFF],
    ];
    for (pixel, color) in source.as_chunks_mut::<4>().0.iter_mut().zip(colors) {
        pixel.copy_from_slice(&color);
    }
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 4,
            active: true,
        },
        ProjectionRow {
            active_x_origin: 2,
            active_width: 4,
            pixel_clock_divisor: 2,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_sgx_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);

    assert_eq!(pixel(&output, 320, 0), colors[2]);
    assert_eq!(pixel(&output, 320, 479), colors[6]);
    assert_eq!(pixel(&output, 0, 479), OPAQUE_BLACK);
    assert_eq!(pixel(&output, 639, 479), OPAQUE_BLACK);
}

#[test]
fn projection_keeps_empty_and_inactive_rows_opaque_black() {
    let source = vec![0x7F; 4 * 2 * 4];
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 1,
            active: false,
        },
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 1,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_sgx_rgba_rows(&source, 4, &rows, None, &mut output);
    assert!(
        output
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == OPAQUE_BLACK)
    );

    project_sgx_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);
    assert_eq!(pixel(&output, 10, 10), OPAQUE_BLACK);
    assert_eq!(pixel(&output, 10, 470), [0x7F; 4]);
}

#[test]
fn fixed_signal_window_preserves_224_margins_and_distinguishes_239_from_240() {
    let first = usize::from(zeff_pce_core::hardware::PCE_SIGNAL_FIRST_ROW);
    let end = usize::from(zeff_pce_core::hardware::PCE_SIGNAL_ROW_END);

    let project = |active_start: usize, active_end: usize, final_color: [u8; 4]| {
        let mut source = vec![0; zeff_pce_core::hardware::PCE_ACTIVE_FRAME_HEIGHT * 4];
        for pixel in source.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&OPAQUE_BLACK);
        }
        let rows = std::array::from_fn::<_, { zeff_pce_core::hardware::PCE_ACTIVE_FRAME_HEIGHT }, _>(
            |line| ProjectionRow {
                active_x_origin: 0,
                active_width: 1,
                pixel_clock_divisor: 4,
                active: (active_start..active_end).contains(&line),
            },
        );
        for line in active_start..active_end {
            source[line * 4..line * 4 + 4].copy_from_slice(&[0x40, line as u8, 0, 0xFF]);
        }
        source[(active_end - 1) * 4..active_end * 4].copy_from_slice(&final_color);
        let mut base = vec![0; PCE_PRESENTED_RGBA_BYTES];
        let mut supergrafx = vec![0; PCE_PRESENTED_RGBA_BYTES];
        project_base_rgba_rows(&source, 1, &rows, Some((first, end)), &mut base);
        project_sgx_rgba_rows(&source, 1, &rows, Some((first, end)), &mut supergrafx);
        assert_eq!(base, supergrafx);
        base
    };

    let mode_224 = project(28, 252, [0xE0, 0, 0, 0xFF]);
    assert_eq!(pixel(&mode_224, 0, 0), OPAQUE_BLACK);
    assert_ne!(pixel(&mode_224, 0, 22), OPAQUE_BLACK);
    assert_eq!(pixel(&mode_224, 0, PCE_PRESENTED_HEIGHT - 1), OPAQUE_BLACK);

    let mode_239 = project(20, 260, [0xEF, 0, 0, 0xFF]);
    assert_ne!(
        pixel(&mode_239, 0, PCE_PRESENTED_HEIGHT - 1),
        [0xEF, 0, 0, 0xFF]
    );

    let mode_240 = project(19, 259, [0xF0, 0, 0, 0xFF]);
    assert_eq!(
        pixel(&mode_240, 0, PCE_PRESENTED_HEIGHT - 1),
        [0xF0, 0, 0, 0xFF]
    );
    assert_ne!(mode_239, mode_240);
}

#[test]
fn standard_host_input_maps_to_two_button_pad() {
    let mut backend = PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("pad.pce")).unwrap();

    backend.set_input(0b1101, 0b1011);

    let pad = backend.machine.devices().controller().device();
    let zeff_pce_core::hardware::ControllerDevice::TwoButton(pad) = pad else {
        panic!("frontend must construct a standard two-button pad");
    };
    assert_eq!(
        pad.buttons(),
        PadButtons::I
            | PadButtons::SELECT
            | PadButtons::RUN
            | PadButtons::RIGHT
            | PadButtons::LEFT
            | PadButtons::DOWN
    );
}

#[test]
fn explicit_console_wiring_overrides_auto_detection() {
    let backend = PceBackend::new_with_console_wiring(
        rom_with_program(&[0xEA]),
        PathBuf::from("override.pce"),
        PceConsoleWiring::TurboGrafx16,
    )
    .unwrap();

    assert_eq!(
        backend.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
}

#[test]
fn curated_wiring_hash_propagates_for_direct_and_archive_paths() {
    let aero_blasters_sha256 = [
        0xD2, 0xFE, 0x59, 0xCF, 0x24, 0x05, 0x3B, 0xBB, 0xB1, 0xB5, 0xDA, 0x25, 0x21, 0xA9, 0x58,
        0xE3, 0x8A, 0x98, 0x1C, 0xCE, 0x9B, 0xAB, 0xA0, 0xAF, 0x82, 0x5E, 0xA5, 0x18, 0x9D, 0x84,
        0x08, 0xDC,
    ];
    let world_court_tennis_sha256 = [
        0x60, 0xC6, 0x9E, 0xE6, 0x80, 0x6A, 0xA6, 0x14, 0x45, 0x95, 0x49, 0x63, 0x3B, 0xDC, 0x72,
        0x8E, 0x10, 0x5F, 0x85, 0x92, 0xE5, 0x35, 0xFC, 0xC7, 0x96, 0xC2, 0x4C, 0xD6, 0xD7, 0x6A,
        0x6B, 0xD1,
    ];
    let magical_chase_sha256 = [
        0xC5, 0xA3, 0x9C, 0x9D, 0x9B, 0x2D, 0x75, 0x32, 0x44, 0x81, 0x6E, 0xAF, 0xD6, 0x8F, 0x50,
        0x4A, 0x85, 0x59, 0x08, 0xEE, 0xBA, 0xB1, 0xB1, 0xC8, 0xFE, 0xA2, 0xBB, 0xF7, 0xA4, 0xA8,
        0x13, 0xC7,
    ];
    let direct = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("magical.pce")),
        None,
        None,
        magical_chase_sha256,
    )
    .unwrap();
    let archive = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::with_source_path(
            PathBuf::from("cards.zip").join("magical.pce"),
            PathBuf::from("cards.zip"),
        ),
        None,
        None,
        magical_chase_sha256,
    )
    .unwrap();
    let explicit_pce = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("magical.pce")),
        Some(PceConsoleWiring::PcEngine),
        None,
        magical_chase_sha256,
    )
    .unwrap();
    let world_court_tennis = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("world-court-tennis.pce")),
        None,
        None,
        world_court_tennis_sha256,
    )
    .unwrap();
    let aero_archive = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::with_source_path(
            PathBuf::from("cards.7z").join("aero.pce"),
            PathBuf::from("cards.7z"),
        ),
        None,
        None,
        aero_blasters_sha256,
    )
    .unwrap();

    assert_eq!(
        direct.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
    assert_eq!(
        world_court_tennis.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
    assert_eq!(
        aero_archive.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
    assert_eq!(
        archive.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
    assert_eq!(archive.source_path(), Path::new("cards.zip"));
    assert_eq!(
        explicit_pce.machine.devices().console_wiring(),
        PceConsoleWiring::PcEngine
    );
}

#[test]
fn debugger_surface_reports_registers_rom_bytes_and_writable_cpu_space() {
    let backend = PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("debug.pce")).unwrap();
    assert_eq!(backend.debug_cpu_snapshot().registers().pc, RESET_PC);
    assert_eq!(backend.debug_peek8(0), 0xEA);
    assert_eq!(
        backend.memory_regions()[0],
        MemoryRegionDescriptor::cpu_address_space(16)
    );
}

#[test]
fn generic_memory_regions_expose_pce_video_state() {
    let mut backend =
        PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("memory.pce")).unwrap();
    backend.machine.devices_mut().vdc_mut().vram_mut()[0] = 0x1234;
    let vce = backend.machine.devices_mut().vce_mut();
    vce.write_port(zeff_pce_core::hardware::VcePort::from_offset(4), 0x56);
    vce.write_port(zeff_pce_core::hardware::VcePort::from_offset(5), 0x01);

    let mut copied = Vec::new();
    backend
        .copy_memory_region("video_ram", &mut copied)
        .unwrap();
    assert_eq!(copied.len(), zeff_pce_core::hardware::VDC_VRAM_BYTES);
    assert_eq!(&copied[..2], &[0x34, 0x12]);

    backend
        .copy_memory_region("palette_ram", &mut copied)
        .unwrap();
    assert_eq!(
        copied.len(),
        zeff_pce_core::hardware::VCE_PALETTE_COLORS * 2
    );
    assert_eq!(&copied[..2], &[0x56, 0x01]);

    backend.copy_memory_region("oam", &mut copied).unwrap();
    assert_eq!(copied.len(), zeff_pce_core::hardware::VDC_SATB_WORDS * 2);

    let mut supergrafx = synthetic_supergrafx_backend();
    supergrafx.machine.devices_mut().vdc_mut().vram_mut()[0] = 0x1122;
    supergrafx
        .machine
        .devices_mut()
        .supergrafx_video_mut()
        .unwrap()
        .vdc2_mut()
        .vram_mut()[0] = 0x3344;
    supergrafx
        .copy_memory_region("video_ram", &mut copied)
        .unwrap();
    assert_eq!(copied.len(), zeff_pce_core::hardware::VDC_VRAM_BYTES * 2);
    assert_eq!(&copied[..2], &[0x22, 0x11]);
    assert_eq!(
        &copied
            [zeff_pce_core::hardware::VDC_VRAM_BYTES..zeff_pce_core::hardware::VDC_VRAM_BYTES + 2],
        &[0x44, 0x33]
    );
    let regions = supergrafx.memory_regions();
    assert_eq!(
        resolve_memory_region(&regions, "video_ram").unwrap().view,
        MemoryRegionView::Aggregate
    );
    assert_eq!(
        resolve_memory_region(&regions, "oam").unwrap().view,
        MemoryRegionView::Aggregate
    );
}

#[test]
fn supergrafx_direct_and_archive_profiles_expose_dynamic_work_ram() {
    let madou_sha256 = [
        0x9B, 0x57, 0xCD, 0xF0, 0xD0, 0xB1, 0x10, 0xF4, 0x12, 0x8B, 0x86, 0x34, 0x19, 0xD5, 0xBE,
        0x99, 0xA3, 0x70, 0x8B, 0xFB, 0x11, 0xCF, 0xBE, 0x16, 0x96, 0xF2, 0x54, 0x49, 0xB9, 0x91,
        0x02, 0x6D,
    ];
    let direct = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("madou.pce")),
        None,
        None,
        madou_sha256,
    )
    .unwrap();
    let mut archive = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::with_source_path(PathBuf::from("madou.pce"), PathBuf::from("cards.zip")),
        None,
        None,
        madou_sha256,
    )
    .unwrap();

    for backend in [&direct, &archive] {
        assert_eq!(
            backend.machine.hardware_topology(),
            zeff_pce_core::hardware::PceHardwareTopology::SuperGrafx
        );
        assert_eq!(
            backend.machine.devices().psg().revision(),
            zeff_pce_core::hardware::PsgRevision::HuC6280A
        );
        assert_eq!(
            backend.system_ram_len(),
            zeff_pce_core::hardware::SUPERGRAFX_WORK_RAM_LEN
        );
        assert_eq!(
            backend
                .memory_regions()
                .iter()
                .find(|region| region.kind == MemoryRegionKind::SystemRam)
                .and_then(|region| region.size),
            Some(zeff_pce_core::hardware::SUPERGRAFX_WORK_RAM_LEN)
        );
    }
    assert_eq!(archive.source_path(), Path::new("cards.zip"));
    let mut ram = Vec::new();
    archive.copy_memory_region("system_ram", &mut ram).unwrap();
    assert_eq!(ram.len(), zeff_pce_core::hardware::SUPERGRAFX_WORK_RAM_LEN);
}

#[test]
fn audio_backend_drains_stereo_psg_samples_at_the_requested_rate() {
    let mut backend =
        PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("audio.pce")).unwrap();
    let psg = backend.machine.devices_mut().psg_mut();
    let port = zeff_pce_core::hardware::PsgPort::from_offset;
    psg.write_port(port(0), 0);
    psg.write_port(port(1), 0xFF);
    psg.write_port(port(5), 0xFF);
    for value in 0..32 {
        psg.write_port(port(6), value);
    }
    psg.write_port(port(2), 1);
    psg.write_port(port(4), 0x9F);
    backend
        .machine
        .devices_mut()
        .advance_master_ticks(1_365 * 262);

    let mut samples = Vec::new();
    backend.drain_audio_samples_into(&mut samples);
    assert_eq!(samples.len(), 734 * 2);
    assert!(samples.iter().any(|sample| *sample != 0.0));
    assert_eq!(backend.audio_topology().unwrap().channels.len(), 6);
}

fn configure_test_tone(backend: &mut PceBackend) {
    let psg = backend.machine.devices_mut().psg_mut();
    let port = zeff_pce_core::hardware::PsgPort::from_offset;
    psg.write_port(port(0), 0);
    psg.write_port(port(1), 0xFF);
    psg.write_port(port(5), 0xFF);
    for value in 0..32 {
        psg.write_port(port(6), value);
    }
    psg.write_port(port(2), 1);
    psg.write_port(port(4), 0x9F);
}

fn run_audio_frames(
    backend: &mut PceBackend,
    frames: usize,
    lines_263: bool,
) -> (Vec<usize>, Vec<f32>) {
    if lines_263 {
        backend
            .machine
            .devices_mut()
            .vce_mut()
            .write_port(zeff_pce_core::hardware::VcePort::from_offset(0), 0x04);
    }
    let mut counts = Vec::with_capacity(frames);
    let mut samples = Vec::new();
    for _ in 0..frames {
        backend.step_frame();
        let mut frame_samples = Vec::new();
        backend.drain_audio_samples_into(&mut frame_samples);
        assert_eq!(frame_samples.len() % 2, 0);
        assert!(frame_samples.iter().all(|sample| sample.is_finite()));
        counts.push(frame_samples.len() / 2);
        samples.extend(frame_samples);
    }
    (counts, samples)
}

#[test]
fn multi_frame_audio_drains_preserve_fractional_cadence_and_continuity() {
    let mut drained_each_frame = PceBackend::new(
        rom_with_program(&[0xEA]),
        PathBuf::from("audio-frames-a.pce"),
    )
    .unwrap();
    let mut drained_once = PceBackend::new(
        rom_with_program(&[0xEA]),
        PathBuf::from("audio-frames-b.pce"),
    )
    .unwrap();
    configure_test_tone(&mut drained_each_frame);
    configure_test_tone(&mut drained_once);

    let (counts, samples) = run_audio_frames(&mut drained_each_frame, 120, false);
    let (_, one_drain_samples) = run_audio_frames(&mut drained_once, 120, false);
    assert_eq!(samples, one_drain_samples);
    assert_eq!(counts.iter().sum::<usize>(), 88_120);
    assert!(counts.iter().all(|count| matches!(count, 734 | 735)));
    assert!(samples.iter().any(|sample| *sample != 0.0));

    let mut lines_263 =
        PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("audio-263.pce")).unwrap();
    configure_test_tone(&mut lines_263);
    let (counts, _) = run_audio_frames(&mut lines_263, 120, true);
    assert_eq!(counts.iter().sum::<usize>(), 88_456);
    assert!(counts.iter().all(|count| matches!(count, 737 | 738)));
}

#[test]
fn sync_output_write_continues_without_a_runtime_fault() {
    let mut backend = PceBackend::new(
        rom_with_program(&[0x03, 0x05, 0x13, 0x10, 0x23, 0x00, 0x80, 0xFE]),
        PathBuf::from("unsupported.pce"),
    )
    .unwrap();
    backend.framebuffer.fill(0xA5);
    backend.step_frame();

    assert_eq!(backend.take_runtime_fault(), None);
    assert_eq!(backend.frame_count, 1);
    assert!(backend.machine.devices().vdc().sync_output().horizontal());
    assert!(!backend.machine.devices().vdc().sync_output().vertical());
    assert!(backend.framebuffer.iter().any(|&byte| byte != 0xA5));
}
