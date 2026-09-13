use super::*;

#[test]
fn divider_edges_keep_native_pre_oscillator_batch_timing() {
    let mut emu = traced(&[0x76], false);
    for _ in 0..2_000 {
        emu.step_instruction();
    }
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.events[0].cycle, 1076);
    assert_eq!(
        trace.events[0].write,
        Write::SequencerClock {
            primary: 0,
            secondary: 1
        }
    );
    assert_eq!(trace.events[1].cycle, 5172);
    assert_eq!(
        trace.events[1].write,
        Write::SequencerClock {
            primary: 1,
            secondary: 0
        }
    );
    assert!(
        trace
            .events
            .iter()
            .all(|event| event.pc == 0 && event.instruction_source == AudioTraceSource::Unknown)
    );
    trace.validate_complete().unwrap();
}

#[test]
fn divider_reset_records_phase_even_without_a_sequencer_edge() {
    for padding in [0, 300] {
        let mut code = vec![0; padding];
        store(&mut code, 0xff04, 0x5a);
        code.push(0x76);
        let mut emu = traced(&code, false);
        run_to_halt(&mut emu);
        let trace = emu.finish_audio_trace().unwrap();
        let index = trace
            .events
            .iter()
            .position(|event| matches!(event.write, Write::DividerReset { .. }))
            .unwrap();
        let event = &trace.events[index];
        assert_eq!(event.cycle, 40 + padding as u64 * 4);
        assert_eq!(
            event.write,
            Write::DividerReset {
                cause: ResetCause::RegisterWrite,
                divider_counter: 0xabc8 + event.cycle as u16,
                apu_bit: padding != 0,
            }
        );
        if padding != 0 {
            assert_eq!(trace.events[index + 1].cycle, event.cycle);
            assert_eq!(
                trace.events[index + 1].write,
                Write::SequencerClock {
                    primary: 1,
                    secondary: 0
                }
            );
        } else {
            assert_eq!(trace.events.len(), 1);
        }
        trace.validate_complete().unwrap();
    }
}

#[test]
fn cgb_speed_changes_keep_base_clock_and_explicit_divider_frozen_delays() {
    let mut emu = traced(
        &[
            0x3e, 1, 0xe0, 0x4d, 0x10, 0, 0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0x3e, 1,
            0xe0, 0x4d, 0x10, 0, 0x76,
        ],
        true,
    );
    run_to_halt(&mut emu);
    let cycles = emu.cycle_count;
    let trace = emu.finish_audio_trace().unwrap();
    let switches = trace
        .events
        .iter()
        .filter(|event| matches!(event.write, Write::SpeedSwitch { .. }))
        .collect::<Vec<_>>();
    assert_eq!(switches.len(), 2);
    assert_eq!(
        (switches[0].cycle, switches[0].write),
        (44, Write::SpeedSwitch { double_speed: true })
    );
    assert_eq!(
        (switches[1].cycle, switches[1].write),
        (
            65_622,
            Write::SpeedSwitch {
                double_speed: false
            }
        )
    );
    let delays = trace
        .events
        .iter()
        .filter_map(|event| match event.write {
            Write::SpeedSwitchDelay { cycles } => Some((event.cycle, cycles)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(delays, [(44, 65_544), (65_622, 65_538)]);
    assert_eq!(
        registers(&trace),
        [
            (65_598, 0xff26, 0x80, Origin::Cpu),
            (65_608, 0xff24, 0x77, Origin::Cpu)
        ]
    );
    assert_eq!(
        trace
            .events
            .iter()
            .filter(|event| matches!(
                event.write,
                Write::DividerReset {
                    cause: ResetCause::SpeedSwitch,
                    ..
                }
            ))
            .count(),
        2
    );
    assert!(
        !trace
            .events
            .iter()
            .any(|event| matches!(event.write, Write::SequencerClock { .. }))
    );
    assert_eq!(trace.end_cycle, cycles);
    assert_eq!(trace.end_cycle, 131_164);
    trace.validate_complete().unwrap();
}

#[test]
fn stop_and_input_resume_bound_the_frozen_clock_interval() {
    for cgb in [false, true] {
        let mut emu = traced(
            &[0x3e, 0x10, 0xe0, 0, 0x10, 0, 0x3e, 0x77, 0xe0, 0x24, 0x76],
            cgb,
        );
        for _ in 0..4 {
            emu.step_instruction();
        }
        assert_eq!(emu.cpu.running, CpuState::Stopped);
        let stopped_at = emu.cycle_count;
        let div = emu.timer_div();
        for _ in 0..512 {
            emu.step_instruction();
        }
        assert_eq!(emu.timer_div(), div);
        let resumed_at = emu.cycle_count;
        emu.set_input(1, 0);
        assert_eq!(emu.cpu.running, CpuState::Running);
        run_to_halt(&mut emu);
        let trace = emu.finish_audio_trace().unwrap();
        let boundaries = trace
            .events
            .iter()
            .filter_map(|event| match event.write {
                Write::Stop { entered } => Some((event.cycle, entered)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(boundaries, [(stopped_at, true), (resumed_at, false)]);
        assert_eq!(resumed_at - stopped_at, 2048);
        assert!(trace.events.iter().any(|event| matches!(
            event.write,
            Write::DividerReset {
                cause: ResetCause::Stop,
                ..
            }
        )));
        assert!(
            !trace
                .events
                .iter()
                .any(|event| event.cycle > stopped_at && event.cycle < resumed_at)
        );
        trace.validate_complete().unwrap();
    }
}

#[test]
fn active_wave_ram_keeps_raw_address_and_native_rejection_or_redirection() {
    for cgb in [false, true] {
        let mut code = Vec::new();
        store(&mut code, 0xff26, 0x80);
        for index in 0..16 {
            store(&mut code, 0xff30 + index, index as u8);
        }
        for (address, value) in [
            (0xff1a, 0x80),
            (0xff1c, 0x20),
            (0xff1d, 0xfd),
            (0xff1e, 0x87),
        ] {
            store(&mut code, address, value);
        }
        for value in 0..32 {
            code.extend_from_slice(&[0x3e, 0x80 | value, 0xe0, 0x3f]);
        }
        code.push(0x76);
        let mut emu = traced(&code, cgb);
        run_to_halt(&mut emu);
        let actual = emu.apu_wave_ram_snapshot();
        let trace = emu.finish_audio_trace().unwrap();
        let mut reconstructed = [0; 16];
        let mut redirected = false;
        let mut rejected = false;
        for event in &trace.events {
            if let Write::WaveRam {
                address,
                value,
                applied_index,
                ..
            } = event.write
            {
                if let Some(index) = applied_index {
                    reconstructed[index as usize] = value;
                    redirected |= index != (address - 0xff30) as u8;
                } else {
                    rejected = true;
                }
            }
        }
        assert_eq!(reconstructed, actual);
        assert!(redirected);
        assert_eq!(rejected, !cgb);
        trace.validate_complete().unwrap();
    }
}

#[test]
fn oam_dma_blocked_audio_writes_never_reach_the_apu_trace() {
    let mut code = Vec::new();
    let hram = [0x3e, 0xc0, 0xe0, 0x46, 0x3e, 0x77, 0xe0, 0x24, 0x76];
    for (index, byte) in hram.into_iter().enumerate() {
        store(&mut code, 0xff80 + index as u16, byte);
    }
    code.extend_from_slice(&[0xc3, 0x80, 0xff]);
    let mut emu = traced(&code, false);
    run_to_halt(&mut emu);
    let trace = emu.finish_audio_trace().unwrap();
    assert!(registers(&trace).is_empty());
    trace.validate_complete().unwrap();
}

#[test]
fn hram_and_banked_cartridge_instruction_sources_use_physical_offsets() {
    let hram = [0x3e, 0x77, 0xe0, 0x24, 0x76];
    let mut code = Vec::new();
    for (index, byte) in hram.into_iter().enumerate() {
        store(&mut code, 0xff80 + index as u16, byte);
    }
    code.extend_from_slice(&[0xc3, 0x80, 0xff]);
    let mut emu = traced(&code, false);
    run_to_halt(&mut emu);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(
        trace.events[0].instruction_source,
        AudioTraceSource::WorkRam { offset: 0x8002 }
    );

    let mut source = rom(&[0x3e, 2, 0xea, 0, 0x20, 0xc3, 0, 0x40], false);
    source.resize(0x10000, 0);
    source[0x147] = 1;
    source[0x148] = 1;
    source[0x8000..0x8005].copy_from_slice(&hram);
    let mut emu = Emulator::from_rom_data(&source, HardwareModePreference::Auto).unwrap();
    emu.reset_and_begin_audio_trace(100).unwrap();
    run_to_halt(&mut emu);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.events[0].pc, 0x4002);
    assert_eq!(
        trace.events[0].instruction_source,
        AudioTraceSource::CartridgeRom {
            offset: 0x8002,
            bit_reversed: false
        }
    );
    trace.validate_complete().unwrap();
}

#[test]
fn dma_blocked_opcode_does_not_claim_cartridge_writer_provenance() {
    let hram = [
        0x3e, 0xc0, 0xe0, 0x46, 0x06, 38, 0x05, 0x20, 0xfd, 0, 0, 0, 0xc3, 0, 2,
    ];
    let mut code = vec![0x31, 0x26, 0xff];
    for (index, byte) in hram.into_iter().enumerate() {
        store(&mut code, 0xff80 + index as u16, byte);
    }
    code.extend_from_slice(&[0xc3, 0x80, 0xff]);
    let mut source = rom(&code, false);
    source[0x38] = 0x76;
    source[0x200] = 0x76;
    let mut emu = Emulator::from_rom_data(&source, HardwareModePreference::Auto).unwrap();
    emu.reset_and_begin_audio_trace(100).unwrap();
    run_to_halt(&mut emu);
    let trace = emu.finish_audio_trace().unwrap();
    let writes = trace
        .events
        .iter()
        .filter(|event| matches!(event.write, Write::Register { .. }))
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 2);
    for event in writes {
        assert_eq!(event.pc, 0x200);
        assert_eq!(event.instruction_source, AudioTraceSource::Unknown);
        assert!(matches!(
            event.write,
            Write::Register {
                origin: Origin::Cpu,
                ..
            }
        ));
    }
    trace.validate_complete().unwrap();
}
