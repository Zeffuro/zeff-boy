use std::sync::atomic::AtomicBool;

use zeff_emu_common::audio_trace::*;

use super::encode_game_boy;

fn trace() -> GameBoyAudioTrace {
    GameBoyAudioTrace {
        generation: 3,
        cycle_hz: 4_194_304,
        cycle_hz_denominator: 1,
        chip: GameBoyTraceChip {
            native_replay: None,
            clock_hz: 4_194_304,
            model: GameBoyTraceModel::Dmg,
            dmg_compatibility: false,
            reset: GameBoyResetState {
                kind: GameBoyResetKind::PowerOn,
                registers: [0; 23],
                wave_ram: [0; 16],
                nr52: 0,
                divider_counter: 0,
                double_speed: false,
            },
        },
        timing: AudioTraceTiming::CpuBusCycleBoundary,
        start: AudioTraceStart::Reset,
        end_cycle: 4_194_304 * 4 + 71,
        events: Vec::new(),
        dropped_events: 0,
        invalidated: None,
    }
}

fn event(cycle: u64, write: GameBoyTraceWrite) -> AudioTraceEvent<GameBoyTraceWrite> {
    AudioTraceEvent {
        cycle,
        pc: 0,
        instruction_source: AudioTraceSource::Unknown,
        write,
    }
}

fn register(cycle: u64, address: u16, value: u8) -> AudioTraceEvent<GameBoyTraceWrite> {
    event(
        cycle,
        GameBoyTraceWrite::Register {
            address,
            value,
            origin: GameBoyTraceOrigin::Cpu,
        },
    )
}

#[test]
fn native_output_bookkeeping_does_not_change_the_vgm_projection() {
    let cancel = AtomicBool::new(false);
    let mut source = trace();
    source.events.push(register(12, 0xff26, 0x80));
    let baseline = encode_game_boy(&source, &cancel).unwrap().capture.unwrap();
    source.chip.native_replay = Some(GameBoyNativeReplay {
        version: 1,
        sample_rate: 48_000,
    });
    for write in [
        GameBoyTraceWrite::NativeDividerPhase { skip_next: false },
        GameBoyTraceWrite::NativeBatch {
            cycles: 4,
            repetitions: 1,
        },
        GameBoyTraceWrite::PcmDrain { frames: 0 },
        GameBoyTraceWrite::NativeOutputChange {
            setting: GameBoyOutputSetting::ChannelMutes,
        },
    ] {
        source.events.push(event(12, write));
    }
    let captured = encode_game_boy(&source, &cancel).unwrap().capture.unwrap();
    assert_eq!(captured.bytes, baseline.bytes);
    assert_eq!(
        serde_json::to_value(captured.metadata).unwrap(),
        serde_json::to_value(baseline.metadata).unwrap()
    );
    source.chip.native_replay = None;
    assert!(encode_game_boy(&source, &cancel).is_err());
}

#[test]
fn power_on_dmg_preserves_raw_writes_absolute_times_and_terminal_wait() {
    let mut source = trace();
    source.events = vec![
        register(12, 0xff26, 0x80),
        register(12, 0xff12, 0xf3),
        register(99, 0xff14, 0x87),
        event(
            4096,
            GameBoyTraceWrite::SequencerClock {
                primary: 0,
                secondary: 1,
            },
        ),
        event(
            5001,
            GameBoyTraceWrite::WaveRam {
                address: 0xff3a,
                value: 0xf1,
                applied_index: Some(10),
                origin: GameBoyTraceOrigin::Cpu,
            },
        ),
        register(4_000_003, 0xff24, 0x77),
    ];
    let original = source.clone();
    let output = encode_game_boy(&source, &AtomicBool::new(false)).unwrap();
    assert_eq!(source, original);
    assert!(output.unavailable.is_empty());
    let capture = output.capture.unwrap();
    let bytes = &capture.bytes;
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    assert_eq!(&bytes[..4], b"Vgm ");
    assert_eq!(word(4) as usize + 4, bytes.len());
    assert_eq!(word(8), 0x171);
    assert_eq!(word(0x80), 4_194_304);
    assert_eq!(word(0x1c), 0);
    let mut position = word(0x34) as usize + 0x34;
    let mut ticks = 0;
    let mut writes = Vec::new();
    while bytes[position] != 0x66 {
        match bytes[position] {
            0xb3 => {
                writes.push((
                    ticks,
                    u16::from(bytes[position + 1]) + 0xff10,
                    bytes[position + 2],
                ));
                position += 3;
            }
            0x61 => {
                ticks += u64::from(u16::from_le_bytes(
                    bytes[position + 1..position + 3].try_into().unwrap(),
                ));
                position += 3;
            }
            opcode => panic!("unexpected opcode {opcode:02x}"),
        }
    }
    assert_eq!(position + 1, bytes.len());
    let mut preamble = vec![(0, 0xff26, 0)];
    preamble.extend((0xff30..=0xff3f).map(|address| (0, address, 0)));
    preamble.extend([0xff11, 0xff16, 0xff1b, 0xff20].map(|address| (0, address, 0)));
    assert_eq!(&writes[..21], preamble);
    assert_eq!(
        &writes[21..],
        &[
            (0, 0xff26, 0x80),
            (0, 0xff12, 0xf3),
            (1, 0xff14, 0x87),
            (5001 * 44_100 / 4_194_304, 0xff3a, 0xf1),
            (4_000_003 * 44_100 / 4_194_304, 0xff24, 0x77),
        ]
    );
    assert_eq!(ticks, source.end_cycle * 44_100 / 4_194_304);
    assert_eq!(u64::from(word(0x18)), ticks);
    assert_eq!(capture.metadata.guest_write_count, 5);
    assert_eq!(capture.metadata.preamble_write_count, 21);
    assert_eq!(
        capture
            .metadata
            .game_boy
            .unwrap()
            .observed_timing_event_count,
        1
    );
    let inspected = crate::vgm::inspect(
        &capture.bytes,
        crate::ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(inspected.warnings.is_empty());
    assert_eq!(inspected.samples, ticks);
    assert_eq!(inspected.chips[0].name, "gameboy_dmg");
}

#[test]
fn valid_native_timing_controls_have_explicit_vgm_refusals() {
    let mut source = trace();
    source.chip.model = GameBoyTraceModel::Cgb;
    source.chip.reset.kind = GameBoyResetKind::PostBoot;
    source.chip.reset.divider_counter = 0x1e9c;
    source.events = vec![
        event(
            0,
            GameBoyTraceWrite::DividerReset {
                cause: GameBoyDividerResetCause::RegisterWrite,
                divider_counter: 0x1ea0,
                apu_bit: true,
            },
        ),
        event(
            0,
            GameBoyTraceWrite::DividerReset {
                cause: GameBoyDividerResetCause::Stop,
                divider_counter: 0,
                apu_bit: false,
            },
        ),
        event(4, GameBoyTraceWrite::Stop { entered: true }),
        event(8, GameBoyTraceWrite::Stop { entered: false }),
        event(12, GameBoyTraceWrite::SpeedSwitch { double_speed: true }),
        event(12, GameBoyTraceWrite::SpeedSwitchDelay { cycles: 65_544 }),
        event(
            70_000,
            GameBoyTraceWrite::WaveRam {
                address: 0xff35,
                value: 8,
                applied_index: Some(0),
                origin: GameBoyTraceOrigin::Cpu,
            },
        ),
    ];
    let output = encode_game_boy(&source, &AtomicBool::new(false)).unwrap();
    assert!(output.capture.is_none());
    assert_eq!(
        output
            .unavailable
            .iter()
            .map(|row| (row.code, row.first_event_index))
            .collect::<Vec<_>>(),
        vec![
            ("cgb_model", None),
            ("post_boot_seed", None),
            ("divider_reset", Some(0)),
            ("stop", Some(2)),
            ("speed_switch", Some(4)),
            ("wave_ram_access", Some(6))
        ]
    );
}

#[test]
fn dmg_post_boot_and_blocked_wave_writes_are_not_silently_projected() {
    let mut source = trace();
    source.chip.reset.kind = GameBoyResetKind::PostBoot;
    source.chip.reset.registers[1] = 0x80;
    source.chip.reset.registers[2] = 0xf3;
    source.chip.reset.registers[0x14] = 0x77;
    source.chip.reset.registers[0x15] = 0xf3;
    source.chip.reset.nr52 = 0x81;
    source.chip.reset.divider_counter = 0xabc8;
    source.events.push(event(
        44,
        GameBoyTraceWrite::WaveRam {
            address: 0xff30,
            value: 1,
            applied_index: None,
            origin: GameBoyTraceOrigin::Cpu,
        },
    ));
    let output = encode_game_boy(&source, &AtomicBool::new(false)).unwrap();
    assert!(output.capture.is_none());
    assert_eq!(output.unavailable[0].code, "post_boot_seed");
    assert_eq!(output.unavailable[1].code, "wave_ram_access");
}

#[test]
fn malformed_or_incomplete_traces_fail_instead_of_becoming_refusal_bundles() {
    let mut mutations: Vec<GameBoyAudioTrace> = Vec::new();
    let mut source = trace();
    source.invalidated = Some(AudioTraceInvalidation::Reset);
    mutations.push(source);
    let mut source = trace();
    source.dropped_events = 1;
    mutations.push(source);
    let mut source = trace();
    source.cycle_hz = 8_388_608;
    mutations.push(source);
    let mut source = trace();
    source.chip.reset.double_speed = true;
    mutations.push(source);
    let mut source = trace();
    source.chip.reset.nr52 = 0x80;
    mutations.push(source);
    let mut source = trace();
    source.chip.reset.divider_counter = 1;
    mutations.push(source);
    let mut source = trace();
    source.events.push(register(3, 0xff27, 0));
    mutations.push(source);
    let mut source = trace();
    source.events.push(event(
        3,
        GameBoyTraceWrite::WaveRam {
            address: 0xff3f,
            value: 0,
            applied_index: Some(16),
            origin: GameBoyTraceOrigin::Cpu,
        },
    ));
    mutations.push(source);
    let mut source = trace();
    source.events.push(event(
        3,
        GameBoyTraceWrite::SequencerClock {
            primary: 0,
            secondary: 0,
        },
    ));
    mutations.push(source);
    let mut source = trace();
    source.events.push(event(
        3,
        GameBoyTraceWrite::SpeedSwitch { double_speed: true },
    ));
    mutations.push(source);
    let mut source = trace();
    let mut write = register(3, 0xff12, 1);
    write.pc = 0x1_0000;
    source.events.push(write);
    mutations.push(source);
    let mut source = trace();
    source.events.push(event(
        3,
        GameBoyTraceWrite::Register {
            address: 0xff12,
            value: 1,
            origin: GameBoyTraceOrigin::CpuInterrupt,
        },
    ));
    source.events[0].pc = 0x150;
    mutations.push(source);
    let mut source = trace();
    source.events = vec![register(4, 0xff12, 0), register(3, 0xff12, 0)];
    mutations.push(source);
    let mut source = trace();
    source
        .events
        .push(register(source.end_cycle + 1, 0xff12, 0));
    mutations.push(source);
    for source in mutations {
        assert!(
            encode_game_boy(&source, &AtomicBool::new(false)).is_err(),
            "{source:?}"
        );
    }
    assert!(encode_game_boy(&trace(), &AtomicBool::new(true)).is_err());
}

#[test]
fn speed_delays_must_fit_the_native_interval() {
    for cycles in [0, 65_537, 65_545, u64::MAX] {
        let mut source = trace();
        source.chip.model = GameBoyTraceModel::Cgb;
        source
            .events
            .push(event(0, GameBoyTraceWrite::SpeedSwitchDelay { cycles }));
        assert!(encode_game_boy(&source, &AtomicBool::new(false)).is_err());
    }
    let mut source = trace();
    source.chip.model = GameBoyTraceModel::Cgb;
    source.end_cycle = 65_544;
    source.events.push(event(
        1,
        GameBoyTraceWrite::SpeedSwitchDelay { cycles: 65_544 },
    ));
    assert!(encode_game_boy(&source, &AtomicBool::new(false)).is_err());
}
