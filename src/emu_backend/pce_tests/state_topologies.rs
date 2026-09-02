use super::super::*;
use super::{RESET_PC, rom_with_program};
use crate::emu_core_trait::EmulatorCore;

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

pub(super) fn synthetic_supergrafx_backend() -> PceBackend {
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
        sram_recovery: Default::default(),
        memory_base_force_flush: false,
        host_persistence_enabled: true,
        tas_load_provenance: None,
    };
    backend.project_presented_frame();
    backend
}

pub(super) fn synthetic_cd_backend(board: PceHuCardBoard) -> PceBackend {
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

#[test]
fn battery_witness_includes_global_memory_base_contents() {
    let mut backend = PceBackend::new(
        rom_with_program(&[0xEA]),
        PathBuf::from("memory-base-witness.pce"),
    )
    .unwrap();
    let hash = |backend: &PceBackend| {
        let components = backend.battery_components();
        let borrowed = components
            .iter()
            .map(|(name, bytes)| (*name, bytes.as_slice()))
            .collect::<Vec<_>>();
        crate::save_paths::recovery_state::canonical_battery_component_hash(&borrowed)
    };
    let before = hash(&backend);
    let mut ram = backend
        .machine
        .devices()
        .controller()
        .memory_base128()
        .ram()
        .to_vec();
    ram[17] ^= 0x80;
    backend
        .machine
        .devices_mut()
        .controller_mut()
        .memory_base128_mut()
        .load_ram(&ram)
        .unwrap();

    assert_ne!(hash(&backend), before);
}
