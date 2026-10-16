use super::super::collect_md_snapshot;
use super::super::memory_tests::{fixture, request};
use super::*;
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::{LoadConfig, Region};

const START: usize = 0x10000;
const USP: u32 = 0x00ff_e000;
const SSP: u32 = 0x00ff_fffc;

fn d_value(index: usize) -> u32 {
    0x89ab_0100 | index as u32
}

fn a_value(index: usize) -> u32 {
    0x7654_0200 | index as u32
}

fn registers_fixture(sr: u16) -> Emulator {
    let mut rom = vec![0; 0x10200];
    rom[..4].copy_from_slice(&SSP.to_be_bytes());
    rom[4..8].copy_from_slice(&(START as u32).to_be_bytes());
    let mut words = vec![0x207c, (USP >> 16) as u16, USP as u16, 0x4e60];
    for index in 0..8 {
        let value = d_value(index);
        words.extend([
            0x203c | (index as u16) << 9,
            (value >> 16) as u16,
            value as u16,
        ]);
    }
    for index in 0..7 {
        let value = a_value(index);
        words.extend([
            0x207c | (index as u16) << 9,
            (value >> 16) as u16,
            value as u16,
        ]);
    }
    words.extend([0x46fc, sr, 0x60fe, 0x4e71]);
    for (index, word) in words.iter().enumerate() {
        rom[START + index * 2..START + index * 2 + 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut emu = Emulator::new(rom, LoadConfig::default()).unwrap();
    for _ in 0..18 {
        emu.step_instruction().unwrap();
    }
    emu
}

fn assert_no_execution_data(cpu: &CpuDebugSnapshot) {
    assert_eq!(cpu.cpu_state, "Read-only");
    assert_eq!(cpu.status_text, "M68000 register snapshot");
    assert_eq!(cpu.last_opcode_line, "Instruction history unavailable");
    assert!(cpu.recent_opcodes.is_empty());
    assert!(cpu.call_stack.is_empty());
    assert!(!cpu.call_stack_available);
    assert!(cpu.breakpoints.is_empty());
    assert!(cpu.one_shot_breakpoints.is_empty());
    assert!(cpu.breakpoint_hit_conditions.is_empty());
    assert!(cpu.supported_events.is_empty());
    assert!(cpu.event_breakpoints.is_empty());
    assert!(cpu.rom_breakpoints.is_empty());
    assert!(cpu.watchpoints.is_empty());
    assert_eq!(cpu.hit_breakpoint, None);
    assert_eq!(cpu.hit_rom_breakpoint, None);
    assert!(cpu.hit_watchpoint.is_none());
    assert_eq!(cpu.hit_event, None);
    assert!(cpu.io_registers.is_empty());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_register_snapshot_formats_all_registers_stacks_and_status_bits() {
    for supervisor in [false, true] {
        for trace in [false, true] {
            for interrupt_mask in 0..8 {
                for ccr in 0..32 {
                    let sr = (u16::from(trace) << 15)
                        | (u16::from(supervisor) << 13)
                        | (interrupt_mask << 8)
                        | ccr;
                    let emu = registers_fixture(sr);
                    let cpu = snapshot(&emu);
                    let mut expected = Vec::new();
                    for index in (0..8).step_by(2) {
                        expected.push(format!(
                            "D{index}:{:08X}  D{}:{:08X}",
                            d_value(index),
                            index + 1,
                            d_value(index + 1)
                        ));
                    }
                    for index in (0..8).step_by(2) {
                        expected.push(format!(
                            "A{index}:{:08X}  A{}:{:08X}",
                            a_value(index),
                            index + 1,
                            if index == 6 {
                                if supervisor { SSP } else { USP }
                            } else {
                                a_value(index + 1)
                            }
                        ));
                    }
                    expected.push("USP:00FFE000  SSP:00FFFFFC".into());
                    expected.push(format!("PC:00010066  SR:{sr:04X}"));
                    assert_eq!(cpu.register_lines, expected);
                    assert_eq!(cpu.pc, 0x10066);
                    assert_eq!(cpu.cycles, emu.cpu_clock().0);
                    assert!(cpu.cycles > 0);
                    let flags = ['X', 'N', 'Z', 'V', 'C'];
                    for (index, name) in flags.into_iter().enumerate() {
                        assert_eq!(cpu.flags[index], (name, ccr & (16 >> index) != 0));
                    }
                    assert_eq!(cpu.sections.len(), 1);
                    assert_eq!(cpu.sections[0].heading, "Status register");
                    assert_eq!(
                        cpu.sections[0].lines,
                        [format!(
                            "Trace:{}  Supervisor:{}  Interrupt mask:{interrupt_mask}",
                            u8::from(trace),
                            u8::from(supervisor)
                        )]
                    );
                    assert_no_execution_data(&cpu);
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_register_snapshot_request_is_opt_in_and_preserves_future_frame_pcm() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            let mut emu = fixture(region, h40);
            let mut control = emu.clone();
            let encoded = emu.encode_state().unwrap();
            let mut req = request();
            req.show_memory_viewer = false;
            assert!(collect_md_snapshot(&emu, &req, None).cpu_debug.is_none());
            req.want_debug_info = true;
            for _ in 0..8 {
                let cpu = collect_md_snapshot(&emu, &req, None).cpu_debug.unwrap();
                assert_eq!(cpu.pc, emu.registers().pc);
                assert_no_execution_data(&cpu);
                assert_eq!(emu, control);
                assert_eq!(emu.encode_state().unwrap(), encoded);
            }
            let end = emu.timing_snapshot().now().get() + region.frame_ticks() * 2;
            let previous_frame = emu.frame_count();
            while emu.timing_snapshot().now().get() < end {
                assert_eq!(
                    emu.step_instruction().unwrap(),
                    control.step_instruction().unwrap()
                );
            }
            assert!(emu.frame_count() > previous_frame);
            assert_eq!(emu, control);
            assert_eq!(emu.framebuffer(), control.framebuffer());
            let pcm = emu.drain_audio_samples();
            assert!(pcm.iter().any(|sample| *sample != 0.0));
            assert_eq!(pcm, control.drain_audio_samples());
            assert_eq!(emu.encode_state().unwrap(), control.encode_state().unwrap());
        }
    }
}
