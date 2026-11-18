use super::*;
use crate::debug::{CpuDisassemblyTarget, DebugTarget, DisassemblyTarget};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sound_target_snapshot_is_read_only_at_real_prefix_deadline_and_halt_boundaries() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut emu = normal_machine(region, timing);
            let mut witnessed = [false; 2];
            for _ in 0..20_000 {
                emu.step_instruction().unwrap();
                let index = if emu.sound_cpu().is_halted() {
                    1
                } else if emu.sound_prefix_pending() {
                    0
                } else {
                    continue;
                };
                if witnessed[index] {
                    continue;
                }
                witnessed[index] = true;
                let encoded = state(&emu);
                let before_pcm = pcm(&emu);
                let before_tick = emu.timing_snapshot();
                let mut req = request();
                req.want_debug_info = true;
                req.show_disassembler = true;
                req.debug_target = DebugTarget::Sound;
                req.debug_revision = 7;
                req.disasm_target = Some(DisassemblyTarget::Cpu(CpuDisassemblyTarget {
                    cpu_address: 1,
                    storage_offset: None,
                    bank: None,
                    thumb: None,
                }));
                let data = collect_md_snapshot(&emu, &req, None);
                let cpu = data.cpu_debug.unwrap();
                assert_eq!(data.debug_target, DebugTarget::Sound);
                assert_eq!(data.debug_revision, 7);
                assert_eq!(cpu.pc, u32::from(emu.sound_cpu().regs().pc));
                assert_eq!(cpu.cycles, emu.sound_cpu().cycles());
                assert!(cpu.status_text.contains("SOUND Z80"));
                assert!(cpu.call_stack.is_empty() && !cpu.call_stack_available);
                assert!(cpu.breakpoints.is_empty() && cpu.watchpoints.is_empty());
                assert!(data.md_timed_ports.is_none());
                let view = data.disassembly_view.unwrap();
                assert_eq!(view.cursor.cpu_address(), Some(1));
                assert_eq!(view.lines[0].mnemonic.as_str(), "LD SP,$1FFE");
                assert_eq!(view.lines[0].bytes.as_slice(), [0x31, 0xfe, 0x1f]);
                req.disasm_target = Some(DisassemblyTarget::Cpu(CpuDisassemblyTarget {
                    cpu_address: 0x3fff,
                    storage_offset: None,
                    bank: None,
                    thumb: None,
                }));
                let view = collect_md_snapshot(&emu, &req, None)
                    .disassembly_view
                    .unwrap();
                assert_eq!(
                    view.lines.last().unwrap().mnemonic.as_str(),
                    "Unknown instruction bytes"
                );
                assert!(view.lines.last().unwrap().bytes.is_empty());
                for address in [0x4000, 0x6000, 0x7f11, 0x8000, 0xffff] {
                    assert!(emu.peek_sound(address).is_none());
                }
                assert_eq!(emu.peek_sound(1), Some(0x31));
                assert_eq!(emu.peek_sound(0x2001), Some(0x31));
                assert_eq!(state(&emu), encoded);
                assert_eq!(pcm(&emu), before_pcm);
                assert_eq!(emu.timing_snapshot(), before_tick);
                if index == 1 {
                    break;
                }
            }
            assert_eq!(witnessed, [true; 2], "{region:?}/{timing:?}");
        }
    }
}
