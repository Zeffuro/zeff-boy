use super::*;
use crate::debug::DebugUiActions;
use crate::emu_thread::{GuestCallRequest, MdTimedPortFailureKind};
use std::sync::Arc;
use zeff_emu_common::debug::WatchType;
use zeff_emu_common::replay::ReplayFirmwareManifest;
use zeff_md_core::emulator::DebugPortWrite;

fn discovery(name: &str) -> Arc<crate::audio_discovery::media::ScanInput> {
    Arc::new(crate::audio_discovery::media::ScanInput {
        #[cfg(not(target_arch = "wasm32"))]
        cdda: None,
        system: None,
        standalone_audio: None,
        bytes: Arc::from([0u8; 1]),
        provenance: None,
        analysis_profile: "test",
        display_name: Some(name.into()),
    })
}
fn firmware(id: &str) -> Vec<ReplayFirmwareManifest> {
    vec![ReplayFirmwareManifest::Skipped {
        firmware_id: id.into(),
        compatibility_version: 1,
    }]
}
fn actions() -> DebugUiActions {
    let mut actions = DebugUiActions::none();
    actions.trace_enabled = Some(true);
    actions.trace_capacity = Some(512);
    actions.trace_clear = true;
    actions.add_breakpoint = Some(0x200);
    actions.add_one_shot_breakpoint = Some(0x202);
    actions.add_breakpoint_after = Some((0x204, 2));
    actions.add_watchpoint = Some((0xff0100, 0xff0103, WatchType::ReadWrite));
    actions.add_rom_breakpoints.push(0x200);
    actions
}
fn request() -> GuestCallRequest {
    GuestCallRequest {
        name: "fenced".into(),
        target: 0x300,
        storage_offset: Some(0x300),
        explicit_overlay: false,
        exec_mode: crate::symbols::ExecMode::M68000,
        instruction_budget: 1,
    }
}
fn ownership(error: anyhow::Error) {
    assert_eq!(
        error.downcast_ref::<MdOwnershipError>(),
        Some(&MdOwnershipError::RollbackOwned)
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn direct_backend_mutation_matrix_is_fenced_then_normal_after_exact_release() {
    for generation in [None, Some(true)] {
        let mut md = backend(false);
        md.step_frame();
        md.audio_generation_enabled = generation;
        md.rewind_audio_changed.set(true);
        md.host_persistence_enabled = true;
        md.set_firmware_manifests(firmware("original"));
        let scan = discovery("original");
        md.set_audio_discovery_input(scan.clone());
        let mut idle = DebugUiActions::none();
        idle.trace_capacity = Some(2048);
        idle.add_breakpoint = Some(0x200);
        md.apply_debug_actions(&idle);
        let mut clear = DebugUiActions::none();
        clear.remove_breakpoints.push(0x200);
        md.apply_debug_actions(&clear);
        let trace_address = md.instruction_trace() as *const _ as usize;
        let trace_capacity = md.instruction_trace().capacity();
        let recovery_address = &md.sram_recovery as *const _ as usize;
        let baseline = md.core().clone();
        let raw = md.encode_state_bytes().unwrap();
        let rewind = md.encode_rewind_checkpoint().unwrap();
        let pointer = address(&md);
        let config = MdRollbackConfig::capture(md.core()).unwrap();
        let owner = md.begin_rollback(&config).unwrap();
        let mut output = vec![f32::from_bits(0x8000_0000), 1.25];
        let expected_output = bits(&output);
        md.set_controllers(ControllerKind::SixButton);
        md.set_input(0x51, 3);
        md.set_input_p2(0x42, 8);
        md.set_sample_rate(22_050);
        md.set_apu_sample_generation_enabled(false);
        md.set_apu_channel_mutes(&[true; 10]);
        md.drain_audio_samples_into(&mut output);
        assert_eq!(bits(&output), expected_output);
        let ram_patch = crate::cheats::CheatPatch::WideRamWrite {
            address: 0xff0040,
            value: crate::cheats::CheatValue::Constant(0x5a),
        };
        let rom_patch = crate::cheats::CheatPatch::WideRomWrite {
            address: 0x300,
            value: crate::cheats::CheatValue::Constant(0xa5),
        };
        md.apply_ram_cheats(&[]);
        md.apply_ram_cheats(&[ram_patch]);
        ownership(md.try_install_rom_patches(&[rom_patch]).unwrap_err());
        ownership(md.reset_and_begin_audio_trace(64).unwrap_err());
        ownership(md.finish_audio_trace().unwrap_err());
        md.reset();
        md.step_frame();
        md.apply_execution_controls(true, true);
        md.debug_suspend();
        assert!(md.take_runtime_fault().is_none());
        assert!(!md.take_rewind_audio_change());
        assert!(md.rewind_audio_changed.get());
        assert_eq!(md.audio_generation_enabled, generation);
        ownership(md.load_state_from_bytes(Vec::new()).unwrap_err());
        ownership(md.load_state_from_bytes(raw.clone()).unwrap_err());
        ownership(md.restore_rewind_checkpoint(&[]).unwrap_err());
        ownership(md.restore_rewind_checkpoint(&rewind).unwrap_err());
        assert_eq!(
            md.try_warm_reset(),
            Err(MdWarmResetError::Ownership(MdOwnershipError::RollbackOwned))
        );
        md.set_opcode_history_enabled(true);
        md.apply_debug_actions(&actions());
        md.apply_rom_breakpoint_actions(&actions());
        md.apply_watchpoint_actions(&actions());
        assert!(matches!(
            md.step_debug_instruction(),
            Err(super::super::super::MdExecutionError::Ownership(_))
        ));
        assert!(matches!(
            md.step_debug_field(),
            Err(super::super::super::MdExecutionError::Ownership(_))
        ));
        let (captured, _) = md.step_captured_instruction();
        assert!(matches!(
            captured,
            Err(super::super::super::MdExecutionError::Ownership(_))
        ));
        assert!(!md.memory_writes_ready());
        assert!(md.validate_memory_writes(&[]).is_err());
        assert!(md.validate_memory_writes(&[(0xff0040, 0xa5)]).is_err());
        md.apply_memory_writes(&[]);
        md.apply_memory_writes(&[(0xff0040, 0xa5)]);
        ownership(md.execute_guest_call(&request()).unwrap_err());
        ownership(md.undo_guest_call(&[]).unwrap_err());
        ownership(md.try_install_rom_patches(&[]).unwrap_err());
        ownership(md.flush_save_ram().unwrap_err());
        assert!(md.battery_components().is_empty());
        md.set_firmware_manifests(firmware("replacement"));
        md.set_audio_discovery_input(discovery("replacement"));
        #[cfg(not(target_arch = "wasm32"))]
        md.set_host_persistence_enabled(true);
        assert!(!md.host_persistence_enabled);
        assert_eq!(md.firmware_manifests(), firmware("original"));
        assert!(Arc::ptr_eq(&md.audio_discovery_input().unwrap(), &scan));
        assert_eq!(md.source_path(), Path::new("backend-rollback.md"));
        assert_eq!(md.instruction_trace() as *const _ as usize, trace_address);
        assert_eq!(md.instruction_trace().capacity(), trace_capacity);
        assert_eq!(&md.sram_recovery as *const _ as usize, recovery_address);
        assert!(md.debugger.rollback_idle());
        same(md.core(), &baseline);
        assert!(md.test_normal_core_mut().is_err());
        let mut backend = EmuBackend::Md(md);
        assert!(backend.try_install_rom_patches(&[]).is_err());
        backend.install_rom_patches(&[]);
        backend.install_rom_patches(&[rom_patch]);
        let error = backend.execute_md_timed_ports(&[]).unwrap_err();
        assert_eq!(error.kind, MdTimedPortFailureKind::Lifecycle);
        let error = backend
            .execute_md_timed_ports(&[DebugPortWrite::Byte {
                address: 0xc00011,
                value: 0x9f,
            }])
            .unwrap_err();
        assert_eq!(error.kind, MdTimedPortFailureKind::Lifecycle);
        assert!(!backend.take_rewind_audio_change());
        assert_eq!(
            backend.md_timed_port_queued_pcm(),
            Some(queued_md(&backend))
        );
        let EmuBackend::Md(mut md) = backend else {
            unreachable!()
        };
        same(md.core(), &baseline);
        md.release_rollback_checked(&owner, None).unwrap();
        assert_eq!(address(&md), pointer);
        assert!(md.host_persistence_enabled);
        assert_eq!(md.audio_generation_enabled, generation);
        assert!(md.take_rewind_audio_change());
        assert!(md.test_normal_core_mut().is_ok());
        let mut pcm = vec![1.25];
        md.drain_audio_samples_into(&mut pcm);
        assert!(pcm.len() > 1);

        md.apply_ram_cheats(&[ram_patch]);
        assert_eq!(md.core().work_ram()[0x40], 0x5a);
        md.try_install_rom_patches(&[rom_patch]).unwrap();
        assert_eq!(md.core().rom_bytes()[0x300], 0x4e);
        assert_eq!(md.core().peek(0x300), Some(0xa5));
        md.try_install_rom_patches(&[]).unwrap();
        md.set_controllers(ControllerKind::SixButton);
        assert_eq!(
            md.core().input().controller_kind(ControllerPort::Two),
            ControllerKind::SixButton
        );
        md.set_input(1, 1);
        md.set_input_p2(2, 4);
        assert_eq!(md.core().input().controller_raw(ControllerPort::One), 0xe7);
        assert_eq!(md.core().input().controller_raw(ControllerPort::Two), 0xde);
        md.set_sample_rate(22_050);
        assert_eq!(md.core().sample_rate(), 22_050);
        md.set_apu_sample_generation_enabled(false);
        assert_eq!(md.audio_generation_enabled, Some(false));
        md.set_apu_channel_mutes(&[true; 10]);
        assert!(md.core().apu_channel_mutes().iter().all(|&mute| mute));
        assert!(md.take_rewind_audio_change());
        md.set_sample_rate(48_000);
        md.set_apu_sample_generation_enabled(true);
        md.set_apu_channel_mutes(&[false; 10]);
        md.reset();
        assert_eq!(md.frame_count(), 0);
        md.step_frame();
        assert_eq!(md.frame_count(), 1);
        md.set_firmware_manifests(firmware("replacement"));
        assert_eq!(md.firmware_manifests(), firmware("replacement"));
        let next_scan = discovery("replacement");
        md.set_audio_discovery_input(next_scan.clone());
        assert!(Arc::ptr_eq(
            &md.audio_discovery_input().unwrap(),
            &next_scan
        ));
        assert!(md.try_install_rom_patches(&[]).is_ok());
        assert!(md.flush_save_ram().unwrap().is_none());
        md.debug_suspend();
        assert!(md.is_suspended());
        assert!(md.memory_writes_ready());
        assert!(md.validate_memory_writes(&[]).is_ok());
        assert!(md.validate_memory_writes(&[(0xff0040, 0xa5)]).is_ok());
        md.apply_memory_writes(&[(0xff0040, 0xa5)]);
        assert_eq!(md.core().work_ram()[0x40], 0xa5);
        md.set_opcode_history_enabled(true);
        md.apply_debug_actions(&actions());
        assert!(!md.debugger.rollback_idle());
        md.set_opcode_history_enabled(false);
        md.set_controllers(ControllerKind::ThreeButton);
        md.load_state_from_bytes(raw.clone()).unwrap();
        assert!(!md.is_suspended());
        md.restore_rewind_checkpoint(&rewind).unwrap();
        assert!(md.try_warm_reset().is_ok());
        md.runtime_fault = Some("retained fault".into());
        assert_eq!(md.take_runtime_fault(), Some("retained fault".into()));
        assert!(md.take_runtime_fault().is_none());
        md.reset_and_begin_audio_trace(64).unwrap();
        assert!(md.finish_audio_trace().unwrap().is_some());
    }
}

fn queued_md(backend: &EmuBackend) -> usize {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    queued(md).len()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn acquisition_rejects_debugger_and_guest_undo_without_clearing_their_contents() {
    let mut md = backend(false);
    let config = MdRollbackConfig::capture(md.core()).unwrap();
    let pointer = address(&md);
    md.debug_suspend();
    assert!(md.begin_rollback(&config).is_err());
    assert!(md.is_suspended());
    md.apply_execution_controls(true, false);
    assert!(!md.is_suspended());
    md.set_opcode_history_enabled(true);
    md.step_debug_instruction().unwrap();
    md.set_opcode_history_enabled(false);
    let baseline = md.core().clone();
    assert!(md.begin_rollback(&config).is_err());
    assert!(!md.debugger.rollback_idle());
    same(md.core(), &baseline);
    assert_eq!(address(&md), pointer);
    md.debugger.clear_session();
    let mut trace = DebugUiActions::none();
    trace.trace_enabled = Some(true);
    md.apply_debug_actions(&trace);
    md.step_debug_instruction().unwrap();
    trace.trace_enabled = Some(false);
    md.apply_debug_actions(&trace);
    assert_eq!(md.instruction_trace().len(), 1);
    let baseline = md.core().clone();
    assert!(md.begin_rollback(&config).is_err());
    assert_eq!(md.instruction_trace().len(), 1);
    same(md.core(), &baseline);
    md.debugger.clear_session();
    md.apply_debug_actions(&actions());
    let baseline = md.core().clone();
    assert!(md.begin_rollback(&config).is_err());
    same(md.core(), &baseline);
    assert!(!md.debugger.rollback_idle());

    let mut md = backend(false);
    let config = MdRollbackConfig::capture(md.core()).unwrap();
    md.debug_suspend();
    let (_, token) = md.execute_guest_call(&request()).unwrap();
    assert!(md.guest_call_undo.is_some());
    md.suspended = false;
    let baseline = md.core().clone();
    assert!(md.begin_rollback(&config).is_err());
    assert!(md.guest_call_undo.is_some());
    same(md.core(), &baseline);
    md.suspended = true;
    md.undo_guest_call(&token).unwrap();
    assert!(md.guest_call_undo.is_none());
}
