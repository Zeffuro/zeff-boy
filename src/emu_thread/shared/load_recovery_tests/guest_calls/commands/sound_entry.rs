use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_guest_call_admitted_entry_sound_fault_restores_exact_machine_and_paused_session() {
    for header in *b"UJE" {
        let mut found = false;
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
                program.extend([0x60fe, 0x4e71]);
                let mut backend = guest_call_backend_with(
                    header,
                    TimingModel::Standard,
                    ControllerKind::ThreeButton,
                    &[(0x200, &program)],
                    false,
                );
                let mut ready = true;
                for _ in 0..setup {
                    if machine_mut(&mut backend).step_instruction().is_err() {
                        ready = false;
                        break;
                    }
                }
                if !ready || machine(&backend).check_debug_call(0x1000).is_err() {
                    continue;
                }
                let mut witness = machine(&backend).clone();
                if !matches!(
                    witness.begin_debug_call(0x1000),
                    Err(zeff_md_core::GuestCallError::Execution(
                        zeff_md_core::MachineFault::Sound(_)
                    ))
                ) {
                    continue;
                }
                assert!(witness.cpu_clock().0 > machine(&backend).cpu_clock().0);
                backend.debug_suspend();
                let mut actions = DebugUiActions::none();
                actions.trace_enabled = Some(true);
                actions.add_one_shot_breakpoint = Some(0x1100);
                configure(&mut backend, &actions);
                let before = machine(&backend).clone();
                let bytes = frontend(&backend);
                let host = session(&mut backend);
                let error = backend
                    .execute_guest_call(&request(0x1000, 8))
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("entry failed") && error.contains("state restored"));
                assert_eq!(machine(&backend), &before);
                assert_eq!(frontend(&backend), bytes);
                assert_eq!(session(&mut backend), host);
                assert!(backend.take_runtime_fault().is_none());
                assert!(backend.undo_guest_call(vec![0; 16]).is_err());
                found = true;
                break 'phase;
            }
        }
        assert!(
            found,
            "no admitted concurrent-SOUND entry fault for {header}"
        );
    }
}
