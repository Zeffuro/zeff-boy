use super::*;
use zeff_md_core::TimingModel;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_rom_breakpoint_actual_workers_stop_after_opcode_and_continue_exactly() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for trace in [false, true] {
            let factory = || {
                guest_call_backend_with(
                    b'U',
                    timing,
                    zeff_md_core::ControllerKind::ThreeButton,
                    &[],
                    false,
                )
            };
            let mut subject = EmuThread::spawn(factory(), false);
            let mut control = EmuThread::spawn(factory(), false);
            let warm = step(&subject, 2, 0, 0);
            let reference = step(&control, 2, 0, 0);
            assert_eq!((warm.advanced_frames, reference.advanced_frames), (2, 2));
            assert!(warm.runtime_fault.is_none() && reference.runtime_fault.is_none());
            assert!(warm.audio_samples.iter().any(|sample| *sample != 0.0));
            assert_eq!(pcm(&warm), pcm(&reference));
            assert_eq!(capture(&subject), capture(&control));
            let before = capture(&subject);
            let mut run = guest_call_input(1);
            run.debug_actions.add_rom_breakpoints =
                vec![u64::from(RETURN_PC), u64::from(RETURN_PC), u64::MAX];
            run.debug_actions.trace_enabled = Some(trace);
            let stopped = guest_call_deliver(&subject, run);
            paused(&stopped);
            let cpu = stopped.ui_data.cpu_debug.as_ref().unwrap();
            assert_eq!(cpu.rom_breakpoints, [u64::from(RETURN_PC)]);
            assert_eq!(cpu.hit_rom_breakpoint, Some(u64::from(RETURN_PC)));
            assert!(
                cpu.hit_breakpoint.is_none()
                    && cpu.hit_watchpoint.is_none()
                    && cpu.hit_event.is_none()
            );
            assert_eq!(
                cpu.recent_opcodes[0].storage_offset,
                Some(u64::from(RETURN_PC))
            );
            assert_eq!(cpu.recent_opcodes[0].bytes, [0x60, 0xfe]);
            let caps = stopped.ui_data.core_features.as_ref().unwrap();
            assert!(caps.supports_rom_breakpoints && caps.supports_debugger);
            assert_ne!(capture(&subject), before);
            let mut stop_control = guest_call_input(1);
            stop_control.debug_actions.add_breakpoint_after = Some((RETURN_PC, 2));
            stop_control.debug_actions.trace_enabled = Some(trace);
            let reference = guest_call_deliver(&control, stop_control);
            paused(&reference);
            let reference_cpu = reference.ui_data.cpu_debug.as_ref().unwrap();
            assert_eq!(reference_cpu.hit_breakpoint, Some(RETURN_PC));
            assert_eq!(reference_cpu.cycles, cpu.cycles);
            assert_eq!(reference_cpu.breakpoint_hit_conditions[0].hits, 2);
            assert_eq!(capture(&subject), capture(&control));
            if trace {
                let record = stopped
                    .ui_data
                    .instruction_trace
                    .as_ref()
                    .unwrap()
                    .entries
                    .last()
                    .unwrap();
                assert_eq!(record.physical_rom_offset, Some(u64::from(RETURN_PC)));
            }
            for _ in 0..2 {
                let saved = capture(&subject);
                let inspect = guest_call_deliver(&subject, guest_call_input(0));
                paused(&inspect);
                assert_eq!(
                    inspect.ui_data.cpu_debug.unwrap().hit_rom_breakpoint,
                    Some(u64::from(RETURN_PC))
                );
                assert_eq!(capture(&subject), saved);
            }
            let mut rearm = guest_call_input(1);
            rearm.debug_continue = true;
            let hit_again = guest_call_deliver(&subject, rearm);
            paused(&hit_again);
            assert_eq!(
                hit_again
                    .ui_data
                    .cpu_debug
                    .as_ref()
                    .unwrap()
                    .hit_rom_breakpoint,
                Some(u64::from(RETURN_PC))
            );
            assert!(hit_again.ui_data.cpu_debug.as_ref().unwrap().cycles > cpu.cycles);
            let mut control_step = guest_call_input(0);
            control_step.debug_step = true;
            let reference = guest_call_deliver(&control, control_step);
            paused(&reference);
            assert_eq!(pcm(&hit_again), pcm(&reference));
            assert_eq!(capture(&subject), capture(&control));
            let mut actual = guest_call_input(1);
            actual
                .debug_actions
                .remove_rom_breakpoints
                .push(u64::from(RETURN_PC));
            actual.debug_continue = true;
            let mut expected = guest_call_input(1);
            expected.debug_actions.remove_breakpoints.push(RETURN_PC);
            expected.debug_continue = true;
            for (left, right) in [
                (actual, expected),
                (guest_call_input(1), guest_call_input(1)),
            ] {
                let actual = guest_call_deliver(&subject, left);
                let expected = guest_call_deliver(&control, right);
                assert_eq!((actual.advanced_frames, expected.advanced_frames), (1, 1));
                assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
                assert!(actual.audio_samples.iter().any(|sample| *sample != 0.0));
                assert_eq!(pcm(&actual), pcm(&expected));
                assert_eq!(capture(&subject), capture(&control));
                assert_eq!(
                    subject.shared_framebuffer().load_full(),
                    control.shared_framebuffer().load_full()
                );
                assert!(
                    actual
                        .ui_data
                        .cpu_debug
                        .unwrap()
                        .hit_rom_breakpoint
                        .is_none()
                );
            }
            subject.shutdown();
            control.shutdown();
        }
    }
}
