use super::super::md_rom_cheat_tests::{codes, update};
use super::*;
use zeff_emu_common::debug::{DebugEvent, WatchType};
use zeff_md_core::TimingModel;

fn physical(offsets: &[u64]) -> DebugUiActions {
    let mut actions = DebugUiActions::none();
    actions.add_rom_breakpoints = offsets.to_vec();
    actions.trace_enabled = Some(true);
    actions
}

fn one(backend: &mut EmuBackend, actions: DebugUiActions) -> FrameResult {
    let mut input = md_audio_tests::input(0, false);
    input.snapshot.want_debug_info = true;
    input.snapshot.show_instruction_trace = true;
    input.debug_step = true;
    input.debug_actions = actions;
    EmuThread::handle_step_frames(
        backend,
        input,
        &[],
        false,
        &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
        &mut 0,
        &mut WorkerRuntimeFault::default(),
    )
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_rom_breakpoints_ignore_extensions_data_faults_and_keep_simultaneous_stops() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut program = vec![0x3039, 0, 0x320, 0x4e71, 0x60fe];
        program.resize(0x92, 0x4e71);
        program[0x90] = 0xa55a;
        let mut backend = fixture_with_timing(b'U', &program, timing);
        backend.debug_suspend();
        let result = one(&mut backend, physical(&[0x202, 0x204, 0x320]));
        assert!(result.runtime_fault.is_none());
        let cpu = result.ui_data.cpu_debug.unwrap();
        assert!(cpu.hit_rom_breakpoint.is_none());
        assert_eq!(cpu.recent_opcodes[0].storage_offset, Some(0x200));
        assert_eq!(machine(&backend).registers().d[0] as u16, 0xa55a);

        let mut backend = fixture_with_timing(b'U', &program, timing);
        backend.debug_suspend();
        let mut actions = physical(&[0x200]);
        actions.add_watchpoint = Some((0x320, 0x321, WatchType::Read));
        let result = one(&mut backend, actions);
        let cpu = result.ui_data.cpu_debug.unwrap();
        assert_eq!(cpu.hit_rom_breakpoint, Some(0x200));
        assert_eq!(cpu.hit_watchpoint.unwrap().address, 0x320);

        let mut backend = fixture_with_timing(b'U', &[0x4afc, 0x4e71], timing);
        assert!(
            update(
                &mut backend,
                &mut Vec::new(),
                codes("000010:0000+000012:0300")
            )
            .is_none()
        );
        backend.debug_suspend();
        let mut actions = physical(&[0x200]);
        actions
            .event_breakpoint_changes
            .push((DebugEvent::Exception, true));
        let result = one(&mut backend, actions);
        assert!(result.runtime_fault.is_none());
        let cpu = result.ui_data.cpu_debug.unwrap();
        assert_eq!(cpu.hit_rom_breakpoint, Some(0x200));
        assert_eq!(cpu.hit_event, Some(DebugEvent::Exception));
        assert_eq!(cpu.recent_opcodes[0].storage_offset, Some(0x200));

        let mut backend = fixture_with_timing(b'U', &[0x3039, 0x00b0, 0, 0x4e71], timing);
        backend.debug_suspend();
        let result = one(&mut backend, physical(&[0x200]));
        assert!(result.runtime_fault.is_some());
        let cpu = result.ui_data.cpu_debug.unwrap();
        assert!(cpu.hit_rom_breakpoint.is_none() && cpu.recent_opcodes.is_empty());
        let trace = result.ui_data.instruction_trace.unwrap();
        let fault = trace.entries.last().unwrap();
        assert_eq!(fault.physical_rom_offset, Some(0x200));
        assert!(fault.boundary.unwrap().machine_fault);

        let mut backend = fixture_with_timing(b'U', &[0x4e72, 0x2700, 0x4e71], timing);
        backend.debug_suspend();
        let first = one(&mut backend, physical(&[0x204]));
        assert!(
            first
                .ui_data
                .cpu_debug
                .unwrap()
                .hit_rom_breakpoint
                .is_none()
        );
        let poll = one(&mut backend, DebugUiActions::none());
        assert!(poll.runtime_fault.is_none());
        assert!(poll.ui_data.cpu_debug.unwrap().hit_rom_breakpoint.is_none());
        assert!(machine(&backend).main_cpu_stopped());

        let mut backend = fixture_with_timing(b'U', &[0x4e72, 0x2000, 0x60fe, 0x4e71], timing);
        machine_mut(&mut backend).step_instruction().unwrap();
        machine_mut(&mut backend).set_interrupt_level(6);
        backend.debug_suspend();
        let mut actions = physical(&[0x204]);
        actions
            .event_breakpoint_changes
            .push((DebugEvent::Interrupt, true));
        let irq = one(&mut backend, actions);
        assert!(irq.runtime_fault.is_none());
        let cpu = irq.ui_data.cpu_debug.unwrap();
        assert_eq!(cpu.hit_event, Some(DebugEvent::Interrupt));
        assert!(cpu.hit_rom_breakpoint.is_none() && cpu.recent_opcodes.is_empty());
        let trace = irq.ui_data.instruction_trace.unwrap();
        assert_eq!(trace.entries.last().unwrap().physical_rom_offset, None);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_rom_breakpoints_keep_overlay_queue_history_and_surface_imported_unknown_until_refill() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for frontend in [false, true] {
            let mut backend = fixture_with_timing(
                b'U',
                &[0x303c, 0x1122, 0x4e71, 0x4e71, 0x60fe, 0x4e71],
                timing,
            );
            backend.debug_suspend();
            let before = machine(&backend).encode_frontend_state().unwrap();
            let mut current = Vec::new();
            assert!(
                update(
                    &mut backend,
                    &mut current,
                    codes("000200:7007+000202:4E71+000204:7208")
                )
                .is_none()
            );
            assert_eq!(machine(&backend).encode_frontend_state().unwrap(), before);
            assert_eq!(machine(&backend).peek(0x200), Some(0x70));
            let old = one(&mut backend, physical(&[0x200, 0x204, 0x208]));
            assert_eq!(
                old.ui_data.cpu_debug.unwrap().hit_rom_breakpoint,
                Some(0x200)
            );
            assert_eq!(machine(&backend).registers().d[0] as u16, 0x1122);
            let raw = machine(&backend).encode_state().unwrap();
            let front = machine(&backend).encode_frontend_state().unwrap();
            assert!(update(&mut backend, &mut current, Vec::new()).is_none());
            assert_eq!(machine(&backend).encode_frontend_state().unwrap(), front);
            let retained = one(&mut backend, DebugUiActions::none());
            assert_eq!(
                retained.ui_data.cpu_debug.unwrap().hit_rom_breakpoint,
                Some(0x204)
            );
            assert_eq!(machine(&backend).registers().d[1], 8);
            if frontend {
                backend.restore_rewind_checkpoint(front).unwrap();
            } else {
                backend.load_state_from_bytes(raw).unwrap();
            }
            backend.debug_suspend();
            let imported = one(&mut backend, DebugUiActions::none());
            let cpu = imported.ui_data.cpu_debug.unwrap();
            assert!(cpu.hit_rom_breakpoint.is_none());
            assert_eq!(cpu.recent_opcodes[0].storage_offset, None);
            assert_eq!(machine(&backend).registers().d[1], 8);
            let second = one(&mut backend, DebugUiActions::none());
            assert_eq!(
                second.ui_data.cpu_debug.unwrap().recent_opcodes[0].storage_offset,
                None
            );
            let refill = one(&mut backend, DebugUiActions::none());
            assert_eq!(
                refill.ui_data.cpu_debug.unwrap().hit_rom_breakpoint,
                Some(0x208)
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_owned_rom_rewind_rejects_corruption_and_restores_known_queue_with_current_overlay() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend =
            fixture_with_timing(b'U', &[0x7001, 0x7202, 0x7403, 0x60fe, 0x4e71], timing);
        let mut fault = WorkerRuntimeFault::default();
        let running = deliver(
            &mut backend,
            1,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
        );
        assert!(running.runtime_fault.is_none());
        let saved = backend.encode_rewind_checkpoint().unwrap();
        assert!(saved.starts_with(b"ZMDDBG01"));
        let mut current = Vec::new();
        assert!(update(&mut backend, &mut current, codes("000206:7009")).is_none());
        let before = backend.encode_rewind_checkpoint().unwrap();
        let mut corrupt = saved.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        for bad in [corrupt, saved[..saved.len() - 1].to_vec()] {
            assert!(backend.restore_rewind_checkpoint(bad).is_err());
            assert_eq!(backend.encode_rewind_checkpoint().unwrap(), before);
        }
        backend.restore_rewind_checkpoint(saved).unwrap();
        assert_eq!(machine(&backend).peek(0x206), Some(0x70));
        let stopped = deliver(
            &mut backend,
            1,
            physical(&[0x206]),
            false,
            false,
            &mut fault,
        );
        assert_eq!(
            stopped.ui_data.cpu_debug.unwrap().hit_rom_breakpoint,
            Some(0x206)
        );
        let next = one(&mut backend, DebugUiActions::none());
        assert_eq!(
            next.ui_data.cpu_debug.unwrap().hit_rom_breakpoint,
            Some(0x206)
        );
        assert_eq!(machine(&backend).registers().d[0], 9);
        backend.reset();
        let reset = deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
        );
        let cpu = reset.ui_data.cpu_debug.unwrap();
        assert_eq!(cpu.rom_breakpoints, [0x206]);
        assert!(cpu.hit_rom_breakpoint.is_none() && cpu.recent_opcodes.is_empty());
    }
}
