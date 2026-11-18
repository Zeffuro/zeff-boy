use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};

pub(in crate::emu_thread) fn backend() -> EmuBackend {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1b0..0x1bc]
        .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 0x3f, 0xff]);
    let words = [
        0x13fc_u16, 0x00a5, 0x0020, 0x0001, 0x13fc, 0x005a, 0x00b0, 0, 0x60fe,
    ];
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = std::path::Path::new("step-persistence.md");
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    )
    .unwrap()
    .backend;
    assert!(backend.save_ram_kind().is_persistent());
    assert_eq!(sram(&backend).len(), 8192);
    assert_ne!(sram(&backend)[0], 0xa5);
    backend.debug_suspend();
    backend
}

pub(in crate::emu_thread) fn sram(backend: &EmuBackend) -> Vec<u8> {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    md.core().dump_save_ram().unwrap()
}

pub(in crate::emu_thread) fn input(step: bool, continue_requested: bool) -> FrameInput {
    let mut input = md_audio_tests::input(0, false);
    input.debug_step = step;
    input.debug_continue = continue_requested;
    input.audio.recording_capture.semantic = true;
    input.snapshot.want_debug_info = true;
    input
}

fn inert_result() -> FrameResult {
    EmuThread::handle_step_frames(
        &mut backend(),
        input(false, false),
        &[],
        false,
        &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
        &mut 10,
        &mut WorkerRuntimeFault::default(),
    )
}

pub(in crate::emu_thread) fn edit_input(writes: &[(u32, u8)]) -> FrameInput {
    let mut request = input(false, false);
    request.audio.recording_capture.active = true;
    request.debug_actions.memory_writes = writes.to_vec();
    request
}

pub(in crate::emu_thread) fn assert_edit_result(result: &FrameResult, ready: bool) {
    assert_eq!(result.advanced_frames, 0);
    assert_eq!(
        result
            .ui_data
            .core_features
            .as_ref()
            .unwrap()
            .memory_editing
            .unwrap()
            .enabled,
        ready
    );
    assert!(result.audio_timeline_discontinuities.is_empty());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_step_persistence_finalizer_separates_instruction_dirty_from_debugger_epochs() {
    for (frames, mutation, step, faulted, dirty, epoch) in [
        (0, false, false, false, false, false),
        (0, false, true, false, true, false),
        (0, false, true, true, true, false),
        (0, false, false, true, false, false),
        (0, true, false, false, true, true),
        (0, true, false, true, true, false),
        (1, false, false, false, true, false),
        (1, false, false, true, true, false),
    ] {
        let mut result = inert_result();
        result.advanced_frames = frames;
        let mut fault = WorkerRuntimeFault::default();
        if faulted {
            fault.latch(Some("partial execution fault".into()));
        }
        let mut pending = Vec::new();
        let mut uncapped = true;
        assert_eq!(
            crate::emu_thread::commands::finalize_step_result(
                &mut result,
                mutation,
                step,
                crate::emu_thread::AudioRecordingCapture {
                    active: true,
                    semantic: true,
                },
                &mut pending,
                &fault,
                &mut uncapped,
            ),
            dirty,
            "frames={frames} mutation={mutation} step={step} faulted={faulted}"
        );
        assert_eq!(uncapped, !faulted);
        assert_eq!(pending.len(), usize::from(epoch));
        if epoch {
            assert_eq!(
                pending[0],
                crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation
            );
        }
        assert!(result.audio_timeline_discontinuities.is_empty());
    }
}
