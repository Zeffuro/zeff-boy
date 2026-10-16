use super::*;
use zeff_emu_common::audio_trace::{
    AudioTraceSource, MdTraceOrigin, MdTraceSource, MdTraceWrite, MdTraceWriter,
};

mod lifecycle;
mod publication;
mod sound;

fn fixture(region: Region, concurrent: bool, program: &[u16]) -> Emulator {
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in program.iter().enumerate() {
        rom[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound: concurrent,
        },
    )
    .unwrap()
}

fn step(machine: &mut Emulator, count: usize) {
    for _ in 0..count {
        machine.step_instruction().unwrap();
    }
}

fn move_immediate(program: &mut Vec<u16>, word: bool, value: u16, address: u32) {
    program.extend([
        if word { 0x33fc } else { 0x13fc },
        value,
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn main_program() -> Vec<u16> {
    let mut program = Vec::new();
    for (word, value, address) in [
        (false, 0x9f, 0xc00010),
        (false, 0x85, 0xc00011),
        (true, 0xab06, 0xc00010),
        (true, 0x100, 0xa11100),
        (true, 0x100, 0xa11200),
        (false, 0x2a, 0xa04000),
        (false, 0x80, 0xa04001),
        (false, 0xb6, 0xa04002),
        (false, 0xc0, 0xa04003),
        (false, 0x99, 0xa07f10),
        (false, 0x9f, 0xa07f11),
        (true, 0, 0xa11200),
        (true, 0, 0xa11200),
    ] {
        move_immediate(&mut program, word, value, address);
    }
    program.push(0x60fe);
    program
}

#[test]
fn main_cpu_ports_preserve_lanes_latches_reset_and_master_time() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            let mut machine = fixture(region, concurrent, &main_program());
            machine.reset_and_begin_audio_trace(32).unwrap();
            assert_eq!(machine.bus.now.get(), 56);
            assert_eq!(machine.psg_snapshot().volumes, [15; 4]);
            let mut boundaries = Vec::new();
            for _ in 0..13 {
                let before = machine.bus.now.get();
                let pc = machine.registers().pc;
                machine.step_instruction().unwrap();
                boundaries.push((before, machine.bus.now.get(), pc));
            }
            let state = machine.encode_state().unwrap();
            let mut untouched = machine.clone();
            let capture = machine.finish_audio_trace().unwrap();
            assert_eq!(machine.encode_state().unwrap(), state);
            assert_eq!(
                machine.drain_audio_samples(),
                untouched.drain_audio_samples()
            );
            assert_eq!(capture.effective_rom_len, 0x4000);
            assert_eq!(
                capture.effective_rom_sha256,
                <[u8; 32]>::from(Sha256::digest(machine.rom_bytes()))
            );
            let trace = capture.trace;
            trace.validate_complete().unwrap();
            assert_eq!(trace.cycle_hz, region.master_hz() as u32);
            assert_eq!(trace.cycle_hz_denominator, 1);
            assert_eq!(trace.chip.psg_divisor, 240);
            assert_eq!(trace.chip.opn2_divisor, 7);
            assert_eq!(trace.timing, AudioTraceTiming::BusServiceBoundary);
            let indices = [1, 2, 5, 6, 7, 8, 10, 11];
            assert_eq!(trace.events.len(), indices.len());
            for (event, index) in trace.events.iter().zip(indices) {
                let (begin, end, pc) = boundaries[index];
                assert!(event.cycle > begin && event.cycle <= end);
                assert_eq!(event.cycle % 7, 0);
                assert_eq!(event.pc, pc);
                assert_eq!(event.instruction_source, AudioTraceSource::Unknown);
                let origin = MdTraceOrigin {
                    writer: MdTraceWriter::M68000,
                    boundary_pc: pc,
                    source: MdTraceSource::EffectiveRom { offset: pc },
                };
                let expected = match index {
                    1 => MdTraceWrite::Psg {
                        origin,
                        value: 0x85,
                    },
                    2 => MdTraceWrite::Psg {
                        origin,
                        value: 0x06,
                    },
                    5 => MdTraceWrite::Opn2Port {
                        origin,
                        port: 0,
                        value: 0x2a,
                    },
                    6 => MdTraceWrite::Opn2Port {
                        origin,
                        port: 1,
                        value: 0x80,
                    },
                    7 => MdTraceWrite::Opn2Port {
                        origin,
                        port: 2,
                        value: 0xb6,
                    },
                    8 => MdTraceWrite::Opn2Port {
                        origin,
                        port: 3,
                        value: 0xc0,
                    },
                    10 => MdTraceWrite::Psg {
                        origin,
                        value: 0x9f,
                    },
                    11 => MdTraceWrite::Opn2Reset { origin },
                    _ => unreachable!(),
                };
                assert_eq!(event.write, expected);
            }
            assert!(!trace.events.iter().any(|event| event.cycle < 56));
            assert!(machine.finish_audio_trace().is_none());
        }
    }
}

fn same_hardware(a: &mut Emulator, b: &mut Emulator) {
    assert_eq!(a.encode_state().unwrap(), b.encode_state().unwrap());
    assert_eq!(a.registers(), b.registers());
    assert_eq!(a.framebuffer(), b.framebuffer());
    assert_eq!(a.drain_audio_samples(), b.drain_audio_samples());
}

#[test]
fn observation_and_overflow_preserve_whole_machine_and_future_output() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            let mut off = fixture(region, concurrent, &main_program());
            let mut on = off.clone();
            let mut overflow = off.clone();
            on.reset_and_begin_audio_trace(32).unwrap();
            overflow.reset_and_begin_audio_trace(1).unwrap();
            for count in [1, 2, 4, 6, 100, 1000] {
                step(&mut off, count);
                step(&mut on, count);
                step(&mut overflow, count);
                same_hardware(&mut off.clone(), &mut on);
                same_hardware(&mut off, &mut overflow);
            }
            let trace = on.finish_audio_trace().unwrap();
            trace.trace.validate_complete().unwrap();
            let lost = overflow.finish_audio_trace().unwrap();
            assert_eq!(lost.trace.events, trace.trace.events[..1]);
            assert_eq!(lost.trace.dropped_events, 7);
            assert!(lost.trace.validate_complete().is_err());
            for _ in 0..300 {
                let expected = off.step_instruction();
                assert_eq!(expected, on.step_instruction());
                assert_eq!(expected, overflow.step_instruction());
            }
            same_hardware(&mut off.clone(), &mut on);
            same_hardware(&mut off, &mut overflow);
        }
    }
}
