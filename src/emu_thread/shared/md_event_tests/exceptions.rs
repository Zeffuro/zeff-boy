use super::*;

mod faults;
mod lifecycle;
mod semantics;

fn exception(enabled: bool) -> DebugUiActions {
    let mut actions = DebugUiActions::none();
    actions.event_breakpoint_changes = vec![(DebugEvent::Exception, enabled)];
    actions
}

fn trap(header: u8, timing: TimingModel) -> EmuBackend {
    let mut backend = fixture_with_timing(header, &[0x4e40, 0x60fe, 0x4e71], timing);
    machine_mut(&mut backend)
        .install_rom_patches(&super::super::md_rom_cheat_tests::codes(
            "000080:0000+000082:0300",
        ))
        .unwrap();
    backend
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_exception_event_reaches_normal_worker_without_history_and_resumes_handler() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend = trap(header, timing);
            let mut expected = machine(&backend).clone();
            assert!(matches!(expected.step_instruction().unwrap().cpu,
                TimedOutcome::Instruction(step) if step.exception == Some(32)));
            assert_eq!(expected.registers().pc, 0x300);
            let mut input = md_audio_tests::input(1, false);
            input.debug_actions = exception(true);
            let mut fault = WorkerRuntimeFault::default();
            let result = EmuThread::handle_step_frames(
                &mut backend,
                input,
                &[],
                false,
                &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
                &mut 10,
                &mut fault,
            );
            compare(&backend, &mut expected, &result, 0);
            assert!(backend.is_suspended());
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert_eq!(
                cpu(&result).supported_events,
                [
                    DebugEvent::Interrupt,
                    DebugEvent::Dma,
                    DebugEvent::Exception
                ]
            );
            assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Exception]);
            assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Exception));
            assert!(cpu(&result).recent_opcodes.is_empty());
            let capabilities = result.ui_data.core_features.as_ref().unwrap();
            assert!(capabilities.supports_debugger && capabilities.supports_guest_calls);
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                true,
                &mut fault,
            );
            compare(&backend, &mut expected, &result, 0);
            assert!(!backend.is_suspended());
            assert_eq!(cpu(&result).hit_event, None);
            backend.debug_suspend();
            expected.step_instruction().unwrap();
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                true,
                false,
                &mut fault,
            );
            compare(&backend, &mut expected, &result, 0);
            assert_eq!(cpu(&result).hit_event, None);
            assert_eq!(cpu(&result).recent_opcodes[0].address, 0x300);
            let frozen = machine(&backend).clone();
            let result = deliver(&mut backend, 0, exception(false), false, false, &mut fault);
            assert_eq!(machine(&backend), &frozen);
            assert!(cpu(&result).event_breakpoints.is_empty());
        }
    }
}

pub(super) fn traced(
    backend: &mut EmuBackend,
    frames: usize,
    actions: DebugUiActions,
    step: bool,
    continue_requested: bool,
    fault: &mut WorkerRuntimeFault,
) -> FrameResult {
    let mut input = md_audio_tests::input(frames, false);
    input.debug_actions = actions;
    input.debug_step = step;
    input.debug_continue = continue_requested;
    input.snapshot.want_debug_info = true;
    input.snapshot.show_disassembler = true;
    input.snapshot.show_instruction_trace = true;
    EmuThread::handle_step_frames(
        backend,
        input,
        &[],
        false,
        &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
        &mut 10,
        fault,
    )
}
