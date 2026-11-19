use super::*;
use zeff_emu_common::debug::{TraceBoundaryKind, WatchType};
use zeff_md_core::emulator::InstructionObserver;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_exception_stack_watch_and_fault_prefix_do_not_fabricate_successful_entry() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for failing in [false, true] {
            let mut backend = trap(b'U', timing);
            if failing {
                machine_mut(&mut backend)
                    .install_rom_patches(&super::super::super::md_rom_cheat_tests::codes(
                        "000080:00B0+000082:0000",
                    ))
                    .unwrap();
            }
            let mut expected = machine(&backend).clone();
            assert_eq!(expected.step_instruction().is_err(), failing);
            let mut actions = exception(true);
            actions.trace_enabled = Some(true);
            actions.add_watchpoint = Some((0xfffffa, 0xfffffb, WatchType::Write));
            let mut fault = WorkerRuntimeFault::default();
            let result = traced(&mut backend, 1, actions, false, false, &mut fault);
            assert_eq!(machine(&backend), &expected);
            assert_eq!(result.runtime_fault.is_some(), failing);
            assert_eq!(
                cpu(&result).hit_event,
                (!failing).then_some(DebugEvent::Exception)
            );
            assert!(cpu(&result).hit_watchpoint.is_some());
            let rows = &result.ui_data.instruction_trace.as_ref().unwrap().entries;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].instruction_bytes(), &[0x4e, 0x40]);
            assert_eq!(rows[0].event, (!failing).then_some(DebugEvent::Exception));
            assert_eq!(rows[0].writes().len(), 3);
            let boundary = rows[0].boundary.unwrap();
            assert_eq!(boundary.machine_fault, failing);
            assert_eq!(
                boundary.kind,
                if failing {
                    TraceBoundaryKind::Fault
                } else {
                    TraceBoundaryKind::Instruction
                }
            );
            assert_eq!(boundary.final_exception, (!failing).then_some(32));
            assert_eq!(cpu(&result).recent_opcodes.len(), usize::from(!failing));
            if failing {
                let frozen = machine(&backend).clone();
                let result = traced(
                    &mut backend,
                    1,
                    DebugUiActions::none(),
                    true,
                    true,
                    &mut fault,
                );
                assert_eq!(machine(&backend), &frozen);
                assert!(!fault.can_step());
                assert!(result.runtime_fault.is_none());
                assert_eq!(cpu(&result).hit_event, None);
            }
        }
    }
}

#[derive(Default)]
struct Boundary(Option<TimedOutcome>);
impl InstructionObserver for Boundary {
    fn cpu_boundary(&mut self, outcome: TimedOutcome) {
        self.0 = Some(outcome);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_exception_completed_cpu_metadata_survives_late_sound_fault_without_success_hit() {
    for header in *b"UJE" {
        let mut found = None;
        'phase: for padding in 0..9 {
            for lead in 0..17 {
                let mut program = vec![0x33fc, 0x100, 0xa1, 0x1100, 0x33fc, 0x100, 0xa1, 0x1200];
                let mut sound = vec![0_u8; padding];
                sound.extend([0x3a, 0, 0x7f, 0x76]);
                for (index, value) in sound.iter().enumerate() {
                    program.extend([0x13fc, u16::from(*value), 0xa0, index as u16]);
                }
                program.extend([0x33fc, 0, 0xa1, 0x1100]);
                let setup = 3 + sound.len() + lead;
                program.extend(std::iter::repeat_n(0x4e71, lead));
                program.extend([0x4e40, 0x60fe, 0x4e71]);
                let mut backend = trap(header, TimingModel::Standard);
                let mut patches =
                    super::super::super::md_rom_cheat_tests::codes("000080:0000+000082:0300");
                for (index, word) in program.iter().enumerate() {
                    let text = format!("{:06X}:{word:04X}", 0x200 + index * 2);
                    patches.extend(super::super::super::md_rom_cheat_tests::codes(&text));
                }
                machine_mut(&mut backend)
                    .install_rom_patches(&patches)
                    .unwrap();
                machine_mut(&mut backend).reset_cpu().unwrap();
                let mut ready = true;
                for _ in 0..setup {
                    if machine_mut(&mut backend).step_instruction().is_err() {
                        ready = false;
                        break;
                    }
                }
                if !ready {
                    continue;
                }
                let mut expected = machine(&backend).clone();
                let mut boundary = Boundary::default();
                let result = expected.step_instruction_traced(&mut boundary);
                if matches!(result, Err(zeff_md_core::MachineFault::Sound(_)))
                    && matches!(boundary.0, Some(TimedOutcome::Instruction(step)) if step.exception == Some(32))
                {
                    found = Some((backend, expected));
                    break 'phase;
                }
            }
        }
        let (mut backend, mut expected) =
            found.expect("bounded SOUND phase with completed trap and late machine fault");
        let mut actions = exception(true);
        actions.trace_enabled = Some(true);
        let result = traced(
            &mut backend,
            1,
            actions,
            false,
            false,
            &mut WorkerRuntimeFault::default(),
        );
        let pcm = expected.drain_audio_samples();
        assert!(
            result
                .audio_samples
                .iter()
                .map(|sample| sample.to_bits())
                .eq(pcm.iter().map(|sample| sample.to_bits()))
        );
        assert!(
            machine(&backend) == &expected,
            "late SOUND fault changed hardware state beyond PCM delivery"
        );
        assert!(result.runtime_fault.is_some());
        assert_eq!(cpu(&result).hit_event, None);
        let rows = &result.ui_data.instruction_trace.as_ref().unwrap().entries;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event, Some(DebugEvent::Exception));
        let boundary = rows[0].boundary.unwrap();
        assert_eq!(boundary.kind, TraceBoundaryKind::Instruction);
        assert_eq!(boundary.final_exception, Some(32));
        assert!(boundary.machine_fault);
    }
}
