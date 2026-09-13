use super::*;

#[test]
fn general_dma_retains_source_context_and_bus_clock_cost() {
    let mut code = Vec::new();
    for (offset, value) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
        store(&mut code, 0x1000 + offset as u16, value);
    }
    for (port, value) in [
        (0x40, 0),
        (0x41, 0x10),
        (0x42, 0),
        (0x44, 0xfc),
        (0x45, 0x3f),
        (0x46, 4),
        (0x47, 0),
        (0x48, 0x80),
    ] {
        out(&mut code, port, value);
    }
    out(&mut code, 0x88, 0xaa);
    code.push(0xf4);
    let mut emu = traced(&code, true);
    run_to_halt(&mut emu);
    assert_eq!(&emu.bus.ram[0x3ffc..0x4000], &[0x12, 0x34, 0x56, 0x78]);
    assert_eq!(emu.bus.cycles - emu.cpu_cycles(), 9);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.end_cycle, emu.bus.cycles);
    let dma = trace
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.write,
                Write::WaveRam {
                    origin: Origin::GeneralDma,
                    ..
                }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(dma.len(), 4);
    for (index, (event, value)) in dma.iter().zip([0x12, 0x34, 0x56, 0x78]).enumerate() {
        assert_eq!(
            **event,
            AudioTraceEvent {
                cycle: 69,
                pc: 0xf0036,
                instruction_source: AudioTraceSource::CartridgeRom {
                    offset: 0x36,
                    bit_reversed: false
                },
                write: Write::WaveRam {
                    address: 0x3ffc + index as u16,
                    value,
                    origin: Origin::GeneralDma
                },
            }
        );
    }
    assert_eq!(trace.events.last().unwrap().cycle, 86);
    assert!(matches!(
        trace.events.last().unwrap().write,
        Write::Register {
            port: 0x88,
            value: 0xaa,
            origin: Origin::Cpu
        }
    ));
    trace.validate_complete().unwrap();
}

#[test]
fn interrupt_stack_has_no_stale_instruction_source() {
    let mut code = vec![0xf4; 0x15];
    code[0x10..].copy_from_slice(&[0xb0, 0x66, 0xe6, 0x88, 0xf4]);
    let mut emu = traced(&code, false);
    emu.step_instruction();
    emu.bus.ram[0x98..0x9c].copy_from_slice(&[0x10, 0, 0, 0xf0]);
    emu.bus.io_write8(0xb0, 0x20);
    emu.bus.io_write8(0xb2, 0x40);
    emu.bus.io[0xb4] = 0x40;
    emu.cpu.flags |= 0x0200;
    let flags = emu.cpu_flags().to_le_bytes();
    assert_eq!(emu.step_instruction(), None);
    assert_eq!(emu.cpu_cycles(), 40);
    run_to_halt(&mut emu);
    let trace = emu.finish_audio_trace().unwrap();
    let expected = [
        (0x1ffe, flags[0]),
        (0x1fff, flags[1]),
        (0x1ffc, 0),
        (0x1ffd, 0xf0),
        (0x1ffa, 0),
        (0x1ffb, 0),
    ];
    assert_eq!(trace.events.len(), 7);
    for (event, (address, value)) in trace.events[..6].iter().zip(expected) {
        assert_eq!(
            *event,
            AudioTraceEvent {
                cycle: 8,
                pc: 0,
                instruction_source: AudioTraceSource::Unknown,
                write: Write::WaveRam {
                    address,
                    value,
                    origin: Origin::CpuInterrupt
                },
            }
        );
    }
    let handler = trace.events.last().unwrap();
    assert_eq!((handler.cycle, handler.pc), (41, 0xf0012));
    assert_eq!(
        handler.instruction_source,
        AudioTraceSource::CartridgeRom {
            offset: 0x12,
            bit_reversed: false,
        }
    );
    assert!(matches!(
        handler.write,
        Write::Register {
            origin: Origin::Cpu,
            ..
        }
    ));
}

#[test]
fn deferred_bank_retirement_cannot_relabel_the_writer() {
    let mut bytes = vec![0xff; 0x20_0000];
    bytes[0x1f0000..0x1f0006].copy_from_slice(&[0xb0, 0, 0xe6, 0xc0, 0xe6, 0x88]);
    bytes[0x0f0006..0x0f000b].copy_from_slice(&[0xb0, 0x77, 0xe6, 0x88, 0xf4]);
    bytes[0x1ffff0..0x1ffff5].copy_from_slice(&[0xea, 0, 0, 0, 0xf0]);
    bytes[0x1ffff6..].fill(0);
    bytes[0x1ffffa] = 4;
    checksum(&mut bytes);
    let mut emu = Emulator::new(&bytes, 48_000).unwrap();
    emu.reset_and_begin_audio_trace(10).unwrap();
    run_to_halt(&mut emu);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.events.len(), 2);
    for (event, (cycle, pc, offset, value)) in trace
        .events
        .iter()
        .zip([(16, 0xf0004, 0x1f0004, 0), (24, 0xf0008, 0x0f0008, 0x77)])
    {
        assert_eq!(
            *event,
            AudioTraceEvent {
                cycle,
                pc,
                instruction_source: AudioTraceSource::CartridgeRom {
                    offset,
                    bit_reversed: false
                },
                write: Write::Register {
                    port: 0x88,
                    value,
                    origin: Origin::Cpu
                },
            }
        );
    }
}

#[test]
fn ram_execution_uses_the_actual_work_or_cartridge_ram_offset() {
    for cartridge_ram in [false, true] {
        let mut bytes = rom(&[0xf4], true);
        bytes[0xfffb] = 2;
        checksum(&mut bytes);
        let mut emu = Emulator::new(&bytes, 48_000).unwrap();
        emu.reset_and_begin_audio_trace(10).unwrap();
        let source = if cartridge_ram {
            emu.bus.cartridge.set_ram_bank(3);
            emu.bus.cartridge.save_data_mut()[0x12..0x15].copy_from_slice(&[0xe6, 0x88, 0xf4]);
            emu.cpu.segments[1] = 0x1000;
            emu.cpu.ip = 0x12;
            AudioTraceSource::CartridgeRam { offset: 0x12 }
        } else {
            emu.bus.ram[0x2000..0x2003].copy_from_slice(&[0xe6, 0x88, 0xf4]);
            emu.cpu.segments[1] = 0;
            emu.cpu.ip = 0x2000;
            AudioTraceSource::WorkRam { offset: 0x2000 }
        };
        run_to_halt(&mut emu);
        let trace = emu.finish_audio_trace().unwrap();
        assert_eq!(trace.events.len(), 1);
        assert_eq!(trace.events[0].instruction_source, source);
    }
}

#[test]
fn unmapped_mono_memory_and_eeprom_window_cannot_claim_code_storage() {
    for (pc, save_kind, open_bus) in [(0x4000, 0, 0x90), (0x10012, 0x10, 0xff)] {
        let mut bytes = rom(&[0xf4], false);
        bytes[0xfffb] = save_kind;
        checksum(&mut bytes);
        let mut emu = Emulator::new(&bytes, 48_000).unwrap();
        emu.reset_and_begin_audio_trace(1).unwrap();
        assert_eq!(emu.cpu_peek8(pc), open_bus);
        emu.bus.begin_audio_trace_instruction(pc);
        emu.bus.io_write8(0x88, 0x5a);
        emu.bus.end_audio_trace_instruction();
        let trace = emu.finish_audio_trace().unwrap();
        assert_eq!(trace.events[0].pc, pc);
        assert_eq!(
            trace.events[0].instruction_source,
            AudioTraceSource::Unmapped
        );
    }
}

#[test]
fn sound_dma_applies_multiple_bytes_after_the_native_apu_batch() {
    let mut emu = traced(&[0xf4], true);
    emu.bus.ram[0x1000..0x1003].copy_from_slice(&[0x11, 0x22, 0x33]);
    emu.bus.io_write8(0x90, 0x22);
    emu.bus.io_write8(0x94, 5);
    sound_dma(&mut emu, 0x1000, 3, 0x83);
    emu.bus.step_cycles(300);
    let mut before = Vec::new();
    emu.drain_audio_samples_into(&mut before);
    assert_eq!(before, vec![0.0; 8]);
    emu.bus.step_cycles(300);
    let mut after = Vec::new();
    emu.drain_audio_samples_into(&mut after);
    assert_eq!(after, vec![(f32::from(0x22_u8) / 255.0) * 0.25; 10]);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(
        dma_events(&trace),
        vec![(300, 0x89, 0x11), (300, 0x89, 0x22), (600, 0x89, 0x33)]
    );
    assert_eq!(emu.io_peek8(0x52) & 0x80, 0);
}

#[test]
fn sound_dma_repeat_hold_and_decrement_keep_transfer_order() {
    for (control, source, length, cycles, values, final_source, final_length) in [
        (
            0x8b,
            0x1000,
            2,
            600,
            vec![0x11, 0x22, 0x11, 0x22],
            0x1002,
            0,
        ),
        (0x87, 0x1000, 3, 300, vec![0, 0], 0x1000, 3),
        (0xc3, 0x1002, 3, 400, vec![0x33, 0x22, 0x11], 0x0fff, 0),
    ] {
        let mut emu = traced(&[0xf4], true);
        emu.bus.ram[0x1000..0x1003].copy_from_slice(&[0x11, 0x22, 0x33]);
        sound_dma(&mut emu, source, length, control);
        emu.bus.step_cycles(cycles);
        let trace = emu.finish_audio_trace().unwrap();
        assert_eq!(
            dma_events(&trace),
            values
                .into_iter()
                .map(|value| (u64::from(cycles), 0x89, value))
                .collect::<Vec<_>>()
        );
        assert_eq!(emu.bus.io_read16(0x4a), final_source);
        assert_eq!(emu.bus.io_read16(0x4e), final_length);
    }
}

#[test]
fn hyper_voice_requires_color_and_distinguishes_manual_and_dma_input() {
    let mut code = Vec::new();
    for (port, value) in [
        (0x64, 0x11),
        (0x65, 0x22),
        (0x66, 0x33),
        (0x67, 0x44),
        (0x68, 0x99),
        (0x69, 0x55),
        (0x6a, 0x80),
        (0x6b, 0x60),
        (0x96, 0xff),
    ] {
        out(&mut code, port, value);
    }
    code.push(0xf4);
    for color in [false, true] {
        let mut emu = traced(&code, color);
        run_to_halt(&mut emu);
        emu.bus.ram[0x1000] = 0x7f;
        sound_dma(&mut emu, 0x1000, 1, 0x93);
        let end = emu.bus.cycles + 128;
        emu.bus.step_cycles(128);
        let trace = emu.finish_audio_trace().unwrap();
        if color {
            assert_eq!(trace.events.len(), 8);
            assert_eq!(dma_events(&trace), vec![(end, 0x69, 0x7f)]);
            assert!(trace.events.iter().any(|event| matches!(
                event.write,
                Write::Register {
                    port: 0x69,
                    value: 0x55,
                    origin: Origin::Cpu
                }
            )));
            let live = emu.bus.apu.save_state();
            assert_eq!(live.hyper_voice_left_output, live.hyper_voice_right_output);
            assert_ne!(live.hyper_voice_left_output, 0);
        } else {
            assert!(trace.events.is_empty());
        }
    }
}
