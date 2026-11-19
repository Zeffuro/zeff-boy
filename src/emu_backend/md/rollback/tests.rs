use super::super::ownership::{MdOwnershipError, MdWarmResetError};
use super::*;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_core_trait::EmulatorCore;
use std::path::Path;
use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset};
use zeff_md_core::emulator::rollback::MdRollbackConfig;
use zeff_md_core::{ControllerKind, ControllerPort, Emulator};

mod fences;

fn write_byte(program: &mut Vec<u16>, address: u32, value: u8) {
    program.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn backend(fault_on_b: bool) -> Box<MdBackend> {
    let mut program = Vec::new();
    for (address, value) in [
        (0xc00011, 0x85),
        (0xc00011, 0x06),
        (0xc00011, 0x90),
        (0xa10009, 0x40),
        (0xa1000b, 0x40),
    ] {
        write_byte(&mut program, address, value);
    }
    let loop_pc = 0x200 + program.len() * 2;
    for (level, offset) in [(0x40, 0), (0, 2)] {
        write_byte(&mut program, 0xa10003, level);
        write_byte(&mut program, 0xa10005, level);
        for port in 0..2u16 {
            program.extend([0x1039 | (port << 9), 0x00a1, 3 + port * 2]);
            program.extend([0x13c0 | port, 0x00ff, 0x0100 + offset + port]);
        }
        if fault_on_b && level == 0x40 {
            program.extend([0x0800, 4, 0x6606, 0x1039, 0x00a1, 0x2000]);
        }
    }
    program.extend([0x4ef9, 0, loop_pc as u16]);
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = b'U';
    rom[0x300..0x302].copy_from_slice(&0x4e75u16.to_be_bytes());
    for (index, word) in program.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = Path::new("backend-rollback.md");
    let EmuBackend::Md(md) = load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            md_controller: ControllerKind::ThreeButton,
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    )
    .unwrap()
    .backend
    else {
        unreachable!()
    };
    md
}

fn bits(audio: &[f32]) -> Vec<u32> {
    audio.iter().map(|value| value.to_bits()).collect()
}
fn address(md: &MdBackend) -> usize {
    md.core() as *const Emulator as usize
}
fn queued(md: &MdBackend) -> Vec<u32> {
    bits(&md.core().clone().drain_audio_samples())
}
fn same(actual: &Emulator, expected: &Emulator) {
    assert!(actual == expected);
    assert_eq!(
        actual.encode_state().unwrap(),
        expected.encode_state().unwrap()
    );
    assert_eq!(
        actual.encode_frontend_state().unwrap(),
        expected.encode_frontend_state().unwrap()
    );
    assert_eq!(actual.timing_snapshot(), expected.timing_snapshot());
    assert_eq!(actual.framebuffer(), expected.framebuffer());
    assert_eq!(actual.input(), expected.input());
    assert_eq!(
        bits(&actual.clone().drain_audio_samples()),
        bits(&expected.clone().drain_audio_samples())
    );
}

fn snapshot_request() -> crate::emu_thread::SnapshotRequest {
    crate::emu_thread::SnapshotRequest {
        debug_target: crate::debug::DebugTarget::Primary,
        debug_revision: 0,
        want_debug_info: true,
        want_perf_info: false,
        any_viewer_open: false,
        any_vram_viewer_open: false,
        show_oam_viewer: false,
        show_apu_viewer: false,
        show_disassembler: false,
        show_rom_info: false,
        show_memory_viewer: false,
        memory_view_start: 0,
        show_rom_viewer: false,
        show_instruction_trace: false,
        trace_cursor: Default::default(),
        rom_view_start: 0,
        last_disasm_pc: None,
        last_disasm_mapping: None,
        disasm_target: None,
        memory_search: None,
        rom_search: None,
        render: crate::emu_thread::RenderSettings {
            color_correction: crate::settings::ColorCorrection::None,
            color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            dmg_palette_preset: Default::default(),
            nes_palette_mode: Default::default(),
            nes_custom_palette: None,
            pce_overscan_mode: Default::default(),
            pce_palette_mode: Default::default(),
            sgb_border_enabled: false,
        },
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn backend_retains_original_box_and_nonempty_pcm_on_every_returned_boundary() {
    let mut md = backend(false);
    let stream = md.instruction_trace_stream();
    md.step_frame();
    md.host_persistence_enabled = true;
    md.rewind_audio_changed.set(true);
    assert_eq!(md.audio_generation_enabled, None);
    let original = md.core().clone();
    let original_address = address(&md);
    let pcm = queued(&md);
    assert!(pcm.iter().any(|&sample| sample != 0));
    let config = MdRollbackConfig::capture(md.core()).unwrap();
    let original_hashes = config.boundary_hashes(md.core()).unwrap();
    let mut other = backend(false);
    other.set_controllers(ControllerKind::SixButton);
    let foreign_config = MdRollbackConfig::capture(other.core()).unwrap();
    assert!(md.begin_rollback(&foreign_config).is_err());
    assert_eq!(md.instruction_trace_stream(), stream);
    assert_eq!(address(&md), original_address);
    assert_eq!(queued(&md), pcm);
    assert!(!md.rollback_owned());
    assert!(md.host_persistence_enabled);
    let owner = md.begin_rollback(&config).unwrap();
    assert!(md.rollback_owned());
    let snapshot = md.capture_rollback(&owner).unwrap();
    assert_eq!(md.rollback_hashes(&owner).unwrap(), original_hashes);
    assert_eq!(
        md.rollback_snapshot_hashes(&owner, &snapshot).unwrap(),
        original_hashes
    );
    let mut into = zeff_md_core::emulator::rollback::MdRetainedAllocations::new();
    md.visit_rollback_core_allocations(&mut into).unwrap();
    snapshot.core.visit_retained_allocations(&mut into).unwrap();
    assert_eq!(
        into.finish()
            .unwrap()
            .count(zeff_md_core::emulator::rollback::MdAllocationKind::CoreLayout),
        3
    );
    drop(into);
    assert_eq!(snapshot.core.field(), md.frame_count());
    let before = md.core().clone();
    assert!(
        md.advance_rollback(&owner, MdRollbackInputs::new([0, 0], &config).unwrap())
            .is_err()
    );
    same(md.core(), &before);
    md.restore_rollback(&owner, &snapshot).unwrap();
    let mut ui = crate::ui::collect_md_snapshot(md.core(), &snapshot_request(), None);
    md.overlay_pc_breakpoints(&mut ui);
    if let Some(view) = ui.md_timed_ports {
        assert!(!view.can_submit);
    }
    assert_eq!(md.framebuffer(), original.framebuffer());
    md.retire_rollback(&owner).unwrap();
    assert!(md.capture_rollback(&owner).is_err());
    assert!(md.restore_rollback(&owner, &snapshot).is_err());
    md.release_rollback_checked(&owner, None).unwrap();
    assert!(md.instruction_trace_stream() > stream);
    let returned_stream = md.instruction_trace_stream();
    assert_eq!(address(&md), original_address);
    same(md.core(), &original);
    assert_eq!(config.boundary_hashes(md.core()).unwrap(), original_hashes);
    assert!(md.host_persistence_enabled);
    assert_eq!(md.audio_generation_enabled, None);
    assert!(md.take_rewind_audio_change());
    assert!(!md.take_rewind_audio_change());
    assert!(md.capture_rollback(&owner).is_err());
    assert!(md.release_rollback_checked(&owner, None).is_err());
    assert_eq!(md.instruction_trace_stream(), returned_stream);
    let mut control = original;
    for (buttons, dpad) in [(1, 1), (0x42, 4), (0x09, 8)] {
        md.set_input(buttons, dpad);
        control
            .input_mut()
            .set_host_pad(ControllerPort::One, buttons, dpad);
        md.step_frame();
        control.step_field().unwrap();
        let mut audio = Vec::new();
        md.drain_audio_samples_into(&mut audio);
        assert_eq!(bits(&audio), bits(&control.drain_audio_samples()));
        same(md.core(), &control);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn backend_six_literal_pad_fields_capture_restore_and_failure_are_transactional() {
    let mut md = backend(false);
    let baseline = md.core().clone();
    let pointer = address(&md);
    let config = MdRollbackConfig::capture(md.core()).unwrap();
    let owner = md.begin_rollback(&config).unwrap();
    let saved = md.capture_rollback(&owner).unwrap();
    let mut control = baseline.clone();
    let fields = [
        ([0x0101, 0x080a], [0xe7, 0x5d], [0x67, 0x5d, 0x33, 0x11]),
        ([0x0402, 0x0241], [0xde, 0xab], [0x5e, 0x6b, 0x32, 0x23]),
        ([0x0101, 0x080a], [0xe7, 0x5d], [0x67, 0x5d, 0x33, 0x11]),
        ([0x0402, 0x0241], [0xde, 0xab], [0x5e, 0x6b, 0x32, 0x23]),
        ([0x0101, 0x080a], [0xe7, 0x5d], [0x67, 0x5d, 0x33, 0x11]),
        ([0x0402, 0x0241], [0xde, 0xab], [0x5e, 0x6b, 0x32, 0x23]),
    ];
    let mut outputs = Vec::new();
    for (words, raw, pins) in fields {
        for (port, raw) in [ControllerPort::One, ControllerPort::Two]
            .into_iter()
            .zip(raw)
        {
            control.input_mut().set_controller_raw(port, raw);
            control.input_mut().set_controller_extra_raw(port, 0x0f);
        }
        control.step_field().unwrap();
        let expected = control.drain_audio_samples();
        let result = md
            .advance_rollback(&owner, MdRollbackInputs::new(words, &config).unwrap())
            .unwrap();
        assert!(result.audio.iter().any(|&sample| sample != 0.0));
        assert_eq!(bits(&result.audio), bits(&expected));
        assert_eq!(result.field, control.pacing_field_count());
        assert_eq!(&md.core().work_ram()[0x100..0x104], &pins);
        same(md.core(), &control);
        outputs.push(bits(&result.audio));
    }
    md.restore_rollback(&owner, &saved).unwrap();
    for ((words, _, _), pcm) in fields.into_iter().zip(outputs) {
        let result = md
            .advance_rollback(&owner, MdRollbackInputs::new(words, &config).unwrap())
            .unwrap();
        assert_eq!(bits(&result.audio), pcm);
    }
    let before = md.core().clone();
    assert!(MdRollbackInputs::new([0x1000, 0], &config).is_err());
    same(md.core(), &before);
    let mut foreign = backend(false);
    let foreign_owner = foreign.begin_rollback(&config).unwrap();
    let foreign_snapshot = foreign.capture_rollback(&foreign_owner).unwrap();
    assert!(md.capture_rollback(&foreign_owner).is_err());
    assert!(md.release_rollback_checked(&foreign_owner, None).is_err());
    assert!(md.restore_rollback(&owner, &foreign_snapshot).is_err());
    same(md.core(), &before);
    foreign
        .release_rollback_checked(&foreign_owner, None)
        .unwrap();
    md.release_rollback_checked(&owner, None).unwrap();
    assert_eq!(address(&md), pointer);
    same(md.core(), &baseline);
    let next = md.begin_rollback(&config).unwrap();
    assert!(md.restore_rollback(&next, &saved).is_err());
    assert!(md.release_rollback_checked(&owner, None).is_err());
    md.release_rollback_checked(&next, None).unwrap();

    let mut faulting = backend(true);
    let baseline = faulting.core().clone();
    let config = MdRollbackConfig::capture(faulting.core()).unwrap();
    let owner = faulting.begin_rollback(&config).unwrap();
    assert!(
        faulting
            .advance_rollback(&owner, MdRollbackInputs::new([1, 0], &config).unwrap())
            .is_err()
    );
    same(faulting.core(), &baseline);
    faulting.retire_rollback(&owner).unwrap();
    faulting.release_rollback_checked(&owner, None).unwrap();
    same(faulting.core(), &baseline);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn dropped_owner_and_failed_verification_remain_fenced_with_original_retained() {
    let mut md = backend(false);
    md.audio_generation_enabled = Some(true);
    md.rewind_audio_changed.set(true);
    md.host_persistence_enabled = true;
    let pointer = address(&md);
    let baseline = md.core().clone();
    let config = MdRollbackConfig::capture(md.core()).unwrap();
    let owner = md.begin_rollback(&config).unwrap();
    let mut detached = md.core().clone();
    detached.reset();
    detached.set_sample_rate(22_050);
    same(md.core(), &baseline);
    drop(owner);
    assert!(md.rollback_owned());
    assert!(md.begin_rollback(&config).is_err());
    assert!(md.test_normal_core_mut().is_err());
    assert!(md.flush_save_ram().is_err());
    assert!(!md.host_persistence_enabled);
    match &md.machine {
        MdMachine::Leased(owned) => assert_eq!(owned.host.original_address, pointer),
        _ => unreachable!(),
    }
    let mut md = backend(false);
    md.audio_generation_enabled = Some(true);
    md.rewind_audio_changed.set(true);
    md.host_persistence_enabled = true;
    let pointer = address(&md);
    let baseline = md.core().clone();
    let owner = md.begin_rollback(&config).unwrap();
    if let MdMachine::Leased(owned) = &mut md.machine {
        owned.host.state.push(0xff);
    }
    assert!(md.release_rollback_checked(&owner, None).is_err());
    assert!(matches!(md.machine, MdMachine::Restoring { .. }));
    assert_eq!(address(&md), pointer);
    same(md.core(), &baseline);
    assert!(md.rollback_owned());
    assert!(!md.host_persistence_enabled);
    assert!(md.test_normal_core_mut().is_err());
    assert!(md.flush_save_ram().is_err());
    assert!(!md.take_rewind_audio_change());
    assert!(md.rewind_audio_changed.get());
    assert_eq!(md.audio_generation_enabled, Some(true));
}

mod sound_trace;
