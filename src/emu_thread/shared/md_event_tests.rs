use super::md_pc_breakpoint_tests::{compare, deliver, fixture_with_timing, machine, machine_mut};
use super::*;
use crate::debug::DebugUiActions;
use zeff_emu_common::debug::DebugEvent;
use zeff_emu_common::time::Reset;
use zeff_m68k::timing::TimedOutcome;
use zeff_md_core::TimingModel;

pub(super) mod dma;
mod exceptions;

fn events(enabled: bool) -> DebugUiActions {
    let mut actions = DebugUiActions::none();
    actions.event_breakpoint_changes = vec![(DebugEvent::Interrupt, enabled)];
    actions
}

fn cpu(result: &FrameResult) -> &crate::debug::CpuDebugSnapshot {
    result.ui_data.cpu_debug.as_ref().unwrap()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_event_irq_stops_after_service_without_history_and_resumes_the_handler() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend =
                fixture_with_timing(header, &[0x4e72, 0x2000, 0x60fe, 0x4e71], timing);
            machine_mut(&mut backend).step_instruction().unwrap();
            machine_mut(&mut backend).set_interrupt_level(6);
            let mut expected = machine(&backend).clone();
            assert!(matches!(
                expected.step_instruction().unwrap().cpu,
                TimedOutcome::Interrupt {
                    level: 6,
                    vector: 30,
                    ..
                }
            ));
            let mut input = md_audio_tests::input(1, false);
            input.debug_actions = events(true);
            assert!(!input.snapshot.want_debug_info);
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
            assert_eq!(expected.registers().pc, 0x300);
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
            assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Interrupt]);
            assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Interrupt));
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
            assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Interrupt]);
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
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_event_irq_definitions_survive_only_successful_timeline_changes_clear_hits() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = fixture_with_timing(b'U', &[0x4e72, 0x2000, 0x60fe, 0x4e71], timing);
        let initial = backend.encode_state_bytes().unwrap();
        let rewind = backend.encode_rewind_checkpoint().unwrap();
        let mut fault = WorkerRuntimeFault::default();
        for restore in 0..3 {
            machine_mut(&mut backend).step_instruction().unwrap();
            machine_mut(&mut backend).set_interrupt_level(6);
            let result = deliver(&mut backend, 1, events(true), false, false, &mut fault);
            assert!(result.runtime_fault.is_none());
            assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Interrupt));
            let frozen = machine(&backend).clone();
            assert!(backend.load_state_from_bytes(vec![0; 32]).is_err());
            assert!(backend.restore_rewind_checkpoint(vec![0; 32]).is_err());
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert_eq!(machine(&backend), &frozen);
            assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Interrupt));
            match restore {
                0 => {
                    backend.load_state_from_bytes(initial.clone()).unwrap();
                }
                1 => backend.restore_rewind_checkpoint(rewind.clone()).unwrap(),
                _ => backend.reset(),
            }
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert_eq!(cpu(&result).hit_event, None);
            assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Interrupt]);
        }
        machine_mut(&mut backend).step_instruction().unwrap();
        machine_mut(&mut backend).set_interrupt_level(6);
        let result = deliver(&mut backend, 1, events(true), false, false, &mut fault);
        assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Interrupt));
        let EmuBackend::Md(md) = &mut backend else {
            unreachable!()
        };
        let reset = md.try_warm_reset();
        if timing == TimingModel::Precise {
            assert!(reset.is_err());
        } else {
            reset.unwrap();
        }
        let result = deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
        );
        assert_eq!(
            cpu(&result).hit_event,
            (timing == TimingModel::Precise).then_some(DebugEvent::Interrupt)
        );
        assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Interrupt]);
        let frozen = machine(&backend).clone();
        let result = deliver(&mut backend, 0, events(false), false, false, &mut fault);
        assert_eq!(machine(&backend), &frozen);
        assert_eq!(cpu(&result).hit_event, None);
        assert!(cpu(&result).event_breakpoints.is_empty());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_event_irq_ignores_masked_lines_stop_polls_and_synchronous_exceptions() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = fixture_with_timing(b'U', &[0x4e72, 0x2700, 0x60fe, 0x4e71], timing);
        machine_mut(&mut backend).set_interrupt_level(6);
        let mut expected = machine(&backend).clone();
        expected.step_field().unwrap();
        let mut fault = WorkerRuntimeFault::default();
        let result = deliver(&mut backend, 1, events(true), false, false, &mut fault);
        compare(&backend, &mut expected, &result, 1);
        assert!(!backend.is_suspended());
        assert_eq!(cpu(&result).hit_event, None);
        assert!(expected.main_cpu_stopped());

        let mut backend = fixture_with_timing(b'U', &[0x4e40, 0x60fe, 0x4e71], timing);
        let mut expected = machine(&backend).clone();
        let boundary = expected.step_instruction().unwrap();
        assert!(
            matches!(boundary.cpu, TimedOutcome::Instruction(step) if step.exception == Some(32))
        );
        backend.debug_suspend();
        let result = deliver(&mut backend, 0, events(true), true, false, &mut fault);
        compare(&backend, &mut expected, &result, 0);
        assert_eq!(cpu(&result).hit_event, None);
        assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Interrupt]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_event_irq_failed_handler_prefetch_reports_fault_without_successful_event() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = fixture_with_timing(b'U', &[0x4e72, 0x2000, 0x60fe, 0x4e71], timing);
        machine_mut(&mut backend)
            .install_rom_patches(&super::md_rom_cheat_tests::codes("000078:00B0+00007A:0000"))
            .unwrap();
        machine_mut(&mut backend).step_instruction().unwrap();
        machine_mut(&mut backend).set_interrupt_level(6);
        let mut expected = machine(&backend).clone();
        assert!(expected.step_instruction().is_err());
        let mut fault = WorkerRuntimeFault::default();
        let result = deliver(&mut backend, 1, events(true), false, false, &mut fault);
        assert!(result.runtime_fault.is_some());
        assert_eq!(machine(&backend), &expected);
        assert_eq!(cpu(&result).hit_event, None);
        assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Interrupt]);
        let frozen = machine(&backend).clone();
        let result = deliver(
            &mut backend,
            1,
            DebugUiActions::none(),
            true,
            true,
            &mut fault,
        );
        assert_eq!(machine(&backend), &frozen);
        assert!(result.runtime_fault.is_none() && !fault.can_step());
        assert_eq!(cpu(&result).hit_event, None);
        assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Interrupt]);
    }
}
