use super::*;
use crate::debug::{CpuDisassemblyTarget, DebugTarget};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sound_worker_snapshot_never_overlays_primary_breakpoints_or_mutates_primary_controls() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend = fixture(header, timing);
            let mut fault = WorkerRuntimeFault::default();
            let mut actions = DebugUiActions::none();
            actions.add_breakpoint = Some(0x10200);
            deliver(&mut backend, 1, actions, false, false, &mut fault);
            let state = machine(&backend).encode_state().unwrap();
            let mut input = super::super::md_audio_tests::input(0, false);
            input.snapshot.want_debug_info = true;
            input.snapshot.show_disassembler = true;
            input.snapshot.debug_target = DebugTarget::Sound;
            input.snapshot.debug_revision = 9;
            input.snapshot.disasm_target = Some(DisassemblyTarget::Cpu(CpuDisassemblyTarget {
                cpu_address: 0x201,
                storage_offset: None,
                bank: None,
                thumb: None,
            }));
            input.debug_actions.target = DebugTarget::Sound;
            input.debug_actions.add_breakpoint = Some(0x333);
            let result = EmuThread::handle_step_frames(
                &mut backend,
                input,
                &[],
                false,
                &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
                &mut 0,
                &mut fault,
            );
            assert!(result.runtime_fault.is_none());
            assert_eq!(result.ui_data.debug_target, DebugTarget::Sound);
            assert_eq!(result.ui_data.debug_revision, 9);
            let cpu = result.ui_data.cpu_debug.as_ref().unwrap();
            assert_eq!(cpu.pc, u32::from(machine(&backend).sound_cpu().regs().pc));
            assert!(cpu.breakpoints.is_empty() && cpu.one_shot_breakpoints.is_empty());
            assert!(cpu.watchpoints.is_empty() && cpu.recent_opcodes.is_empty());
            assert!(cpu.hit_breakpoint.is_none() && cpu.hit_watchpoint.is_none());
            assert!(result.ui_data.md_timed_ports.is_none());
            let view = result.ui_data.disassembly_view.as_ref().unwrap();
            assert_eq!(view.cursor.cpu_address(), Some(0x201));
            assert_eq!(view.lines[0].mnemonic.as_str(), "NOP");
            assert!(view.breakpoints.is_empty());
            assert_eq!(machine(&backend).encode_state().unwrap(), state);
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            let cpu = result.ui_data.cpu_debug.unwrap();
            assert_eq!(cpu.breakpoints, [0x10200]);
            assert!(!cpu.breakpoints.contains(&0x333));
            assert_eq!(machine(&backend).encode_state().unwrap(), state);
        }
    }
}
