use super::*;
use crate::hardware::types::CpuState;
use crate::hardware::types::hardware_mode::HardwareModePreference;
use zeff_emu_common::audio_trace::{
    AudioTraceSource, AudioTraceStart, GameBoyDividerResetCause as ResetCause, GameBoyResetKind,
    GameBoyTraceModel, GameBoyTraceOrigin as Origin, GameBoyTraceWrite as Write,
    MAX_AUDIO_TRACE_EVENTS,
};

mod equivalence;
mod invalidation;
mod timing;

fn rom(code: &[u8], cgb: bool) -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    rom[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 0x01]);
    rom[0x143] = if cgb { 0x80 } else { 0 };
    rom[0x150..0x150 + code.len()].copy_from_slice(code);
    rom
}

fn emulator(code: &[u8], cgb: bool) -> Emulator {
    Emulator::from_rom_data(&rom(code, cgb), HardwareModePreference::Auto).unwrap()
}

fn traced(code: &[u8], cgb: bool) -> Emulator {
    let mut emu = emulator(code, cgb);
    emu.reset_and_begin_audio_trace(20_000).unwrap();
    emu
}

fn run_to_halt(emu: &mut Emulator) {
    for _ in 0..10_000 {
        if emu.cpu.running == CpuState::Halted {
            return;
        }
        emu.step_instruction();
    }
    panic!("synthetic program did not halt");
}

fn store(code: &mut Vec<u8>, address: u16, value: u8) {
    let [lo, hi] = address.to_le_bytes();
    code.extend_from_slice(&[0x3e, value, 0xea, lo, hi]);
}

fn registers(trace: &GameBoyAudioTrace) -> Vec<(u64, u16, u8, Origin)> {
    trace
        .events
        .iter()
        .filter_map(|event| match event.write {
            Write::Register {
                address,
                value,
                origin,
            } => Some((event.cycle, address, value, origin)),
            _ => None,
        })
        .collect()
}

#[test]
fn reset_metadata_distinguishes_post_boot_and_firmware_execution() {
    for cgb in [false, true] {
        for firmware in [false, true] {
            let source = rom(&[0x76], cgb);
            let mut emu = if firmware {
                Emulator::from_rom_data_with_boot_rom(
                    &source,
                    HardwareModePreference::Auto,
                    &vec![0; if cgb { 0x900 } else { 0x100 }],
                )
                .unwrap()
            } else {
                emulator(&[0x76], cgb)
            };
            emu.write_byte(0xff30, 0xff);
            emu.step_instruction();
            emu.reset_and_begin_audio_trace(8).unwrap();
            let chip = emu.bus.audio_trace_chip();
            let trace = emu.finish_audio_trace().unwrap();
            assert_eq!(trace.chip, chip);
            assert_eq!(trace.start, AudioTraceStart::Reset);
            assert_eq!(trace.timing, AudioTraceTiming::CpuBusCycleBoundary);
            assert_eq!((trace.cycle_hz, trace.cycle_hz_denominator), (4_194_304, 1));
            assert_eq!(
                trace.chip.model,
                if cgb {
                    GameBoyTraceModel::Cgb
                } else {
                    GameBoyTraceModel::Dmg
                }
            );
            assert_eq!(
                trace.chip.reset.kind,
                if firmware {
                    GameBoyResetKind::PowerOn
                } else {
                    GameBoyResetKind::PostBoot
                }
            );
            assert_eq!(
                trace.chip.reset.nr52,
                if !cgb && !firmware { 0x81 } else { 0 }
            );
            assert_eq!(trace.chip.reset.wave_ram, [0; 16]);
            assert_eq!(
                trace.chip.reset.divider_counter,
                if firmware {
                    0
                } else if cgb {
                    0x1e9c
                } else {
                    0xabc8
                }
            );
            assert_eq!(trace.end_cycle, 0);
            assert!(trace.events.is_empty());
            trace.validate_complete().unwrap();
        }
    }
}

#[test]
fn register_and_wave_writes_use_cpu_bus_cycle_completion() {
    let mut emu = traced(
        &[
            0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x5a, 0xea, 0x30, 0xff, 0x3e, 0x77, 0xe0, 0x24, 0x76,
        ],
        false,
    );
    run_to_halt(&mut emu);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(
        registers(&trace),
        [
            (36, 0xff26, 0x80, Origin::Cpu),
            (80, 0xff24, 0x77, Origin::Cpu)
        ]
    );
    assert_eq!(trace.events[1].cycle, 60);
    assert_eq!(
        trace.events[1].write,
        Write::WaveRam {
            address: 0xff30,
            value: 0x5a,
            applied_index: Some(0),
            origin: Origin::Cpu
        }
    );
    assert_eq!(
        trace
            .events
            .iter()
            .map(|event| event.pc)
            .collect::<Vec<_>>(),
        [0x152, 0x156, 0x15b]
    );
    for event in &trace.events {
        assert_eq!(
            event.instruction_source,
            AudioTraceSource::CartridgeRom {
                offset: u64::from(event.pc),
                bit_reversed: false
            }
        );
    }
    assert_eq!(trace.end_cycle, 84);
    trace.validate_complete().unwrap();
}

#[test]
fn powered_off_register_writes_remain_raw_bus_evidence() {
    let mut code = Vec::new();
    store(&mut code, 0xff26, 0);
    store(&mut code, 0xff12, 0xff);
    store(&mut code, 0xff11, 0x3f);
    code.push(0x76);
    let mut emu = traced(&code, false);
    run_to_halt(&mut emu);
    assert_eq!(emu.apu_regs_snapshot()[2], 0);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(
        registers(&trace)
            .iter()
            .map(|event| (event.1, event.2))
            .collect::<Vec<_>>(),
        [(0xff26, 0), (0xff12, 0xff), (0xff11, 0x3f)]
    );
    trace.validate_complete().unwrap();
}

#[test]
fn firmware_writer_mapping_survives_boot_unmapping() {
    let mut firmware = vec![0; 0x100];
    firmware[..7].copy_from_slice(&[0x3e, 0x80, 0xe0, 0x26, 0xc3, 0xfc, 0x00]);
    firmware[0xfc..].copy_from_slice(&[0x3e, 1, 0xe0, 0x50]);
    let source = rom(&[0x3e, 0x77, 0xe0, 0x24, 0x76], false);
    let mut emu =
        Emulator::from_rom_data_with_boot_rom(&source, HardwareModePreference::Auto, &firmware)
            .unwrap();
    emu.reset_and_begin_audio_trace(100).unwrap();
    run_to_halt(&mut emu);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(
        trace.events[0].instruction_source,
        AudioTraceSource::BootRom { offset: 2 }
    );
    assert_eq!(
        trace.events[1].instruction_source,
        AudioTraceSource::CartridgeRom {
            offset: 0x152,
            bit_reversed: false
        }
    );
    trace.validate_complete().unwrap();
}

#[test]
fn interrupt_stack_audio_writes_have_interrupt_origin() {
    let mut source = rom(
        &[
            0xf3, 0x3e, 0, 0xe0, 0x0f, 0x31, 0x26, 0xff, 0x3e, 1, 0xea, 0xff, 0xff, 0xe0, 0x0f,
            0xfb, 0, 0x76,
        ],
        false,
    );
    source[0x40] = 0x76;
    let mut emu = Emulator::from_rom_data(&source, HardwareModePreference::Auto).unwrap();
    emu.reset_and_begin_audio_trace(100).unwrap();
    run_to_halt(&mut emu);
    let trace = emu.finish_audio_trace().unwrap();
    let writes = registers(&trace);
    assert_eq!(writes.len(), 2);
    assert_eq!(
        (writes[0].1, writes[0].2, writes[0].3),
        (0xff25, 1, Origin::CpuInterrupt)
    );
    assert_eq!(
        (writes[1].1, writes[1].2, writes[1].3),
        (0xff24, 0x61, Origin::CpuInterrupt)
    );
    assert_eq!(writes[1].0 - writes[0].0, 4);
    for event in &trace.events {
        assert_eq!(event.pc, 0);
        assert_eq!(event.instruction_source, AudioTraceSource::Unknown);
    }
    trace.validate_complete().unwrap();
}
