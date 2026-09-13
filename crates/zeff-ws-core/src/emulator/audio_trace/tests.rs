use super::*;
use crate::hardware::cartridge::compute_footer_checksum;
use crate::hardware::cpu::CpuState;
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceSource, AudioTraceStart, MAX_AUDIO_TRACE_EVENTS,
    WonderSwanTraceOrigin as Origin, WonderSwanTraceWrite as Write,
};

mod equivalence;
mod invalidation;
mod timing;

fn checksum(rom: &mut [u8]) {
    let end = rom.len();
    let sum = compute_footer_checksum(rom);
    rom[end - 2..].copy_from_slice(&sum.to_le_bytes());
}

fn rom(code: &[u8], color: bool) -> Vec<u8> {
    let mut bytes = vec![0xff; 0x10000];
    bytes[..code.len()].copy_from_slice(code);
    bytes[0xfff0..0xfff5].copy_from_slice(&[0xea, 0, 0, 0, 0xf0]);
    bytes[0xfff6..].fill(0);
    bytes[0xfff7] = u8::from(color);
    bytes[0xfffa] = 1;
    checksum(&mut bytes);
    bytes
}

fn emulator(code: &[u8], color: bool) -> Emulator {
    Emulator::new(&rom(code, color), 48_000).unwrap()
}

fn traced(code: &[u8], color: bool) -> Emulator {
    let mut emu = emulator(code, color);
    emu.reset_and_begin_audio_trace(20_000).unwrap();
    emu
}

fn out(code: &mut Vec<u8>, port: u8, value: u8) {
    code.extend_from_slice(&[0xb0, value, 0xe6, port]);
}

fn store(code: &mut Vec<u8>, address: u16, value: u8) {
    let [lo, hi] = address.to_le_bytes();
    code.extend_from_slice(&[0x26, 0xc6, 0x06, lo, hi, value]);
}

fn run_to_halt(emu: &mut Emulator) {
    for _ in 0..2_000 {
        if emu.cpu_state() == CpuState::Halted {
            return;
        }
        emu.step_instruction();
        assert_eq!(emu.last_trap(), None);
        assert_ne!(emu.cpu_state(), CpuState::Suspended);
    }
    panic!("synthetic program did not halt");
}

fn sound_dma(emu: &mut Emulator, source: u16, length: u16, control: u8) {
    emu.bus.io_write16(0x4a, source);
    emu.bus.io_write16(0x4c, 0);
    emu.bus.io_write16(0x4e, length);
    emu.bus.io_write16(0x50, 0);
    emu.bus.io_write8(0x52, control);
}

fn dma_events(trace: &WonderSwanAudioTrace) -> Vec<(u64, u16, u8)> {
    trace
        .events
        .iter()
        .filter_map(|event| match event.write {
            Write::Register {
                port,
                value,
                origin: Origin::SoundDma,
            } => {
                assert_eq!(event.pc, 0);
                assert_eq!(event.instruction_source, AudioTraceSource::Unknown);
                Some((event.cycle, port, value))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn reset_metadata_matches_live_apu_and_wave_memory() {
    for color in [false, true] {
        let mut emu = emulator(&[0xf4], color);
        emu.bus.ram[..0x4000].fill(0xa5);
        for port in 0x80..=0x95 {
            emu.bus.apu.write8(port, 0xff);
        }
        emu.bus.apu.write8(0x6a, 0x80);
        emu.bus.apu.write8(0x69, 0x7f);
        emu.bus.step_cycles(200);
        emu.reset_and_begin_audio_trace(1).unwrap();

        let live = emu.bus.apu.save_state();
        let trace = emu.finish_audio_trace().unwrap();
        let reset = &trace.chip.reset;
        assert_eq!(trace.start, AudioTraceStart::Reset);
        assert_eq!(trace.timing, AudioTraceTiming::BusServiceBoundary);
        assert_eq!((trace.cycle_hz, trace.cycle_hz_denominator), (3_072_000, 1));
        assert_eq!(trace.chip.clock_hz, 3_072_000);
        assert_eq!(trace.chip.color, color);
        assert_eq!(trace.end_cycle, 0);
        assert!(trace.events.is_empty());
        assert_eq!(reset.wave_ram, emu.bus.ram[..0x4000]);
        assert_eq!(reset.period_counters, live.period_counter);
        assert_eq!(reset.sample_positions, live.sample_pos);
        assert_eq!(reset.sweep_divider, live.sweep_8192_divider);
        assert_eq!(reset.sweep_counter, live.sweep_counter);
        assert_eq!(reset.hyper_voice_next_left, live.hyper_voice_next_left);
        for (index, &value) in reset.registers.iter().enumerate() {
            let port = 0x80 + index as u16;
            let actual = if port == 0x91 {
                live.output_control
            } else {
                emu.bus.apu.read8(port)
            };
            assert_eq!(value, actual, "reset port {port:02x}");
        }
        for (index, &value) in reset.hyper_voice_registers.iter().enumerate() {
            assert_eq!(value, emu.bus.apu.read8(0x64 + index as u16));
        }
        trace.validate_complete().unwrap();
    }
}

#[test]
fn raw_cpu_writes_include_future_wave_page_and_preserve_noise_reset() {
    let mut code = Vec::new();
    out(&mut code, 0x92, 0x5a);
    out(&mut code, 0x93, 0x21);
    out(&mut code, 0x8e, 0x08);
    store(&mut code, 0x3fff, 0xab);
    store(&mut code, 0x4000, 0x91);
    out(&mut code, 0x8f, 0xff);
    code.push(0xf4);
    for color in [false, true] {
        let mut emu = traced(&code, color);
        run_to_halt(&mut emu);
        assert_eq!(emu.bus.apu.save_state().nreg, 0);
        assert_eq!(emu.io_peek8(0x8e), 0);
        assert_eq!(emu.cpu_peek8(0x3fff), 0xab);
        assert_eq!(emu.cpu_peek8(0x4000), if color { 0x91 } else { 0x90 });
        let trace = emu.finish_audio_trace().unwrap();
        let expected = [
            (
                9,
                2,
                Write::Register {
                    port: 0x92,
                    value: 0x5a,
                    origin: Origin::Cpu,
                },
            ),
            (
                17,
                6,
                Write::Register {
                    port: 0x93,
                    value: 0x21,
                    origin: Origin::Cpu,
                },
            ),
            (
                25,
                10,
                Write::Register {
                    port: 0x8e,
                    value: 0x08,
                    origin: Origin::Cpu,
                },
            ),
            (
                32,
                12,
                Write::WaveRam {
                    address: 0x3fff,
                    value: 0xab,
                    origin: Origin::Cpu,
                },
            ),
            (
                35,
                26,
                Write::Register {
                    port: 0x8f,
                    value: 0xff,
                    origin: Origin::Cpu,
                },
            ),
        ];
        assert_eq!(trace.events.len(), expected.len());
        for (event, (cycle, offset, write)) in trace.events.iter().zip(expected) {
            assert_eq!(
                *event,
                AudioTraceEvent {
                    cycle,
                    pc: 0xf0000 + offset,
                    instruction_source: AudioTraceSource::CartridgeRom {
                        offset: u64::from(offset),
                        bit_reversed: false,
                    },
                    write,
                }
            );
        }
        assert_eq!(trace.end_cycle, 44);
        trace.validate_complete().unwrap();
    }
}

#[test]
fn word_io_retains_both_raw_bytes_at_one_boundary() {
    let mut emu = traced(&[0xb8, 0x12, 0x34, 0xe7, 0x80, 0xf4], true);
    run_to_halt(&mut emu);
    assert_eq!(emu.io_peek8(0x81), 4);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(
        trace
            .events
            .iter()
            .map(|event| (event.cycle, event.write))
            .collect::<Vec<_>>(),
        vec![
            (
                9,
                Write::Register {
                    port: 0x80,
                    value: 0x12,
                    origin: Origin::Cpu
                }
            ),
            (
                9,
                Write::Register {
                    port: 0x81,
                    value: 0x34,
                    origin: Origin::Cpu
                }
            ),
        ]
    );
}
