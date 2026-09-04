use super::*;
use crate::emu_core_trait::EmulatorCore;
use zeff_emu_common::memory::resolve_memory_region;
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_pce_core::hardware::POPULOUS_HUCARD_RAM_LEN;

#[path = "pce_tests/projection.rs"]
mod projection;
#[path = "pce_tests/state_topologies.rs"]
mod state_topologies;

const RESET_PC: u16 = 0xE000;

fn rom_with_program(program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[..program.len()].copy_from_slice(program);
    rom[0x1FFE..0x2000].copy_from_slice(&RESET_PC.to_le_bytes());
    rom
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
        sram_recovery: Default::default(),
        memory_base_force_flush: false,
        host_persistence_enabled: true,
        tas_load_provenance: None,
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
    let title = backend.canonical_title_metadata().unwrap();
    assert_eq!(title.id, "pce-cd:jp:lemmings");
    assert_eq!(title.title, "Lemmings");

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
fn native_mouse_input_updates_buttons_and_motion_on_controller_lines() {
    let mut backend =
        PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("mouse.pce")).unwrap();
    backend.set_pce_mouse_state(PceControllerMode::Mouse, 0x2D, -0x17, 0x03);

    {
        let controller = backend.machine.devices_mut().controller_mut();
        controller.write_lines(true, false);
        controller.write_lines(true, true);
        controller.write_lines(true, false);
        assert_eq!(controller.read_nibble(), 0x02);
        controller.write_lines(false, false);
        assert_eq!(controller.read_nibble(), 0x0C);
        controller.write_lines(true, true);
        controller.write_lines(true, false);
        assert_eq!(controller.read_nibble(), 0x0D);
        controller.write_lines(false, false);
        assert_eq!(controller.read_nibble(), 0x0C);
        controller.write_lines(true, true);
        controller.write_lines(true, false);
        assert_eq!(controller.read_nibble(), 0x0E);
        controller.write_lines(false, false);
        assert_eq!(controller.read_nibble(), 0x0C);
        controller.write_lines(true, true);
        controller.write_lines(true, false);
        assert_eq!(controller.read_nibble(), 0x09);
        controller.write_lines(false, false);
        assert_eq!(controller.read_nibble(), 0x0C);
    }

    backend.set_pce_mouse_state(PceControllerMode::Mouse, 0, 0, 0);
    assert_eq!(backend.machine.devices().controller().read_nibble(), 0x0F);
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

    let controller = backend.machine.devices_mut().controller_mut();
    controller.write_lines(true, false);
    controller.write_lines(true, true);
    controller.write_lines(true, false);
    for (expected_high, expected_low) in [
        (0x0F, 0x0E),
        (0x0F, 0x0D),
        (0x0F, 0x0B),
        (0x0F, 0x07),
        (0x0B, 0x0F),
    ] {
        assert_eq!(controller.read_nibble(), expected_high);
        controller.write_lines(false, false);
        assert_eq!(controller.read_nibble(), expected_low);
        controller.write_lines(true, false);
    }
    assert_eq!(controller.read_nibble(), 0);
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
    let mut backend = state_topologies::synthetic_cd_backend(PceHuCardBoard::SystemCardV1V2);
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

#[test]
fn restored_clean_memory_base_state_forces_one_exact_flush() {
    let temp_dir = unique_temp_dir("memory-base-state-restore");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let memory_base_path = temp_dir.join("mb128.sav");
    let mut backend = state_topologies::synthetic_cd_backend(PceHuCardBoard::SystemCardV1V2);
    backend.paths = BackendPaths::new(temp_dir.join("disc.cue"));
    backend.update_memory_base_mode(PceMemoryBaseMode::Enabled);

    let restored = vec![0xA5; MEMORY_BASE128_RAM_LEN];
    backend.load_memory_base128(&restored).unwrap();
    let state = backend.encode_state_bytes().unwrap();

    memory_base_write_byte(&mut backend, 0x3C);
    backend
        .flush_memory_base128_to_path(&memory_base_path)
        .unwrap();
    assert_ne!(std::fs::read(&memory_base_path).unwrap(), restored);

    backend.load_state_from_bytes(state).unwrap();
    assert!(backend.memory_base_force_flush);
    assert!(
        !backend
            .machine
            .devices()
            .controller()
            .memory_base128()
            .is_dirty()
    );
    assert_eq!(
        backend
            .flush_memory_base128_to_path(&memory_base_path)
            .unwrap(),
        Some(memory_base_path.display().to_string())
    );
    assert_eq!(std::fs::read(&memory_base_path).unwrap(), restored);
    assert_eq!(
        backend
            .flush_memory_base128_to_path(&memory_base_path)
            .unwrap(),
        None
    );

    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn restored_disconnected_memory_base_leaves_primary_untouched() {
    let temp_dir = unique_temp_dir("memory-base-disconnected-restore");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let memory_base_path = temp_dir.join("mb128.sav");
    let primary = vec![0x3C; MEMORY_BASE128_RAM_LEN];
    std::fs::write(&memory_base_path, &primary).unwrap();
    let mut backend = state_topologies::synthetic_cd_backend(PceHuCardBoard::SystemCardV1V2);
    backend.update_memory_base_mode(PceMemoryBaseMode::Disabled);
    let state = backend.encode_state_bytes().unwrap();

    backend.load_state_from_bytes(state).unwrap();
    assert!(!backend.memory_base_force_flush);

    assert_eq!(
        backend
            .flush_memory_base128_to_path(&memory_base_path)
            .unwrap(),
        None
    );
    assert_eq!(std::fs::read(&memory_base_path).unwrap(), primary);

    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn dirty_memory_base_flushes_after_disconnect() {
    let temp_dir = unique_temp_dir("memory-base-dirty-disconnect");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let memory_base_path = temp_dir.join("mb128.sav");
    let mut backend = state_topologies::synthetic_cd_backend(PceHuCardBoard::SystemCardV1V2);
    backend.update_memory_base_mode(PceMemoryBaseMode::Enabled);
    memory_base_write_byte(&mut backend, 0xA5);
    backend.update_memory_base_mode(PceMemoryBaseMode::Disabled);

    assert_eq!(
        backend
            .flush_memory_base128_to_path(&memory_base_path)
            .unwrap(),
        Some(memory_base_path.display().to_string())
    );
    assert_eq!(std::fs::read(&memory_base_path).unwrap()[0], 0xA5);

    std::fs::remove_dir_all(temp_dir).unwrap();
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
        None,
        magical_chase_sha256,
    )
    .unwrap();
    let explicit_pce = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("magical.pce")),
        Some(PceConsoleWiring::PcEngine),
        None,
        None,
        magical_chase_sha256,
    )
    .unwrap();
    let world_court_tennis = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("world-court-tennis.pce")),
        None,
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

    let mut supergrafx = state_topologies::synthetic_supergrafx_backend();
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
        None,
        madou_sha256,
    )
    .unwrap();
    let mut archive = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::with_source_path(PathBuf::from("madou.pce"), PathBuf::from("cards.zip")),
        None,
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
