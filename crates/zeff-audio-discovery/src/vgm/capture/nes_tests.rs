use std::sync::atomic::AtomicBool;

use zeff_emu_common::audio_trace::*;

use super::encode_nes;

const NTSC: (u64, u32) = (19_687_500, 11);
const PAL: (u64, u32) = (53_203_425, 32);
const DENDY: (u64, u32) = (3_546_895, 2);

fn trace(region: NesTraceRegion) -> NesAudioTrace {
    let (numerator, denominator) = match region {
        NesTraceRegion::Ntsc => NTSC,
        NesTraceRegion::Pal => PAL,
        NesTraceRegion::Dendy => DENDY,
    };
    NesAudioTrace {
        generation: 6,
        cycle_hz: numerator as u32,
        cycle_hz_denominator: denominator,
        chip: NesTraceChip {
            clock_hz_numerator: numerator,
            clock_hz_denominator: denominator,
            region,
            reset: NesTraceReset::ZeffPowerOnV1,
            initial_cpu_cycle: 7,
            initial_cpu_cycle_odd: true,
            initial_apu_frame_cycle: 9,
            initial_half_rate_timer_clock: false,
        },
        timing: AudioTraceTiming::CpuBusCycleBoundary,
        start: AudioTraceStart::Reset,
        end_cycle: 32,
        events: Vec::new(),
        dropped_events: 0,
        invalidated: None,
    }
}

fn event(cycle: u64, write: NesTraceWrite) -> AudioTraceEvent<NesTraceWrite> {
    AudioTraceEvent {
        cycle,
        pc: 0x8123,
        instruction_source: AudioTraceSource::CartridgeRom {
            offset: 0x123,
            bit_reversed: false,
        },
        write,
    }
}

fn register(cycle: u64, address: u16, value: u8) -> AudioTraceEvent<NesTraceWrite> {
    event(
        cycle,
        NesTraceWrite::Register {
            address,
            value,
            odd_cycle: cycle.is_multiple_of(2),
        },
    )
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

struct Decoded {
    writes: Vec<(u64, u16, u8)>,
    waits: Vec<u16>,
}

fn decode(bytes: &[u8]) -> Decoded {
    let mut offset = 0x100;
    let mut samples = 0;
    let mut writes = Vec::new();
    let mut waits = Vec::new();
    while bytes[offset] != 0x66 {
        match bytes[offset] {
            0xb4 => {
                writes.push((
                    samples,
                    u16::from(bytes[offset + 1]) + 0x4000,
                    bytes[offset + 2],
                ));
                offset += 3;
            }
            0x61 => {
                let wait = u16::from_le_bytes(bytes[offset + 1..offset + 3].try_into().unwrap());
                assert_ne!(wait, 0);
                samples += u64::from(wait);
                waits.push(wait);
                offset += 3;
            }
            opcode => panic!("unexpected opcode {opcode:02x}"),
        }
    }
    assert_eq!(offset + 1, bytes.len());
    Decoded { writes, waits }
}

#[test]
fn power_on_register_projection_preserves_absolute_times_and_status_observations() {
    let mut source = trace(NesTraceRegion::Ntsc);
    source.events = vec![
        register(0, 0x4011, 0x2a),
        event(
            2,
            NesTraceWrite::StatusRead {
                value: 0,
                origin: NesTraceOrigin::Cpu,
            },
        ),
        register(NTSC.0, 0x4017, 0x80),
        event(
            NTSC.0,
            NesTraceWrite::StatusRead {
                value: 0,
                origin: NesTraceOrigin::Cpu,
            },
        ),
        register(NTSC.0, 0x4015, 0),
    ];
    source.end_cycle = 2 * NTSC.0;
    let original = source.clone();
    let output = encode_nes(&source, &AtomicBool::new(false)).unwrap();
    assert_eq!(source, original);
    assert!(output.unavailable.is_empty());
    let capture = output.capture.unwrap();
    assert_eq!(&capture.bytes[..4], b"Vgm ");
    assert_eq!(word(&capture.bytes, 0x84), 1_789_773);
    assert_eq!(word(&capture.bytes, 0x18), capture.metadata.total_samples);
    assert_eq!(
        decode(&capture.bytes).writes,
        vec![
            (0, 0x4015, 0),
            (0, 0x4011, 0x2a),
            (485_100, 0x4017, 0x80),
            (485_100, 0x4015, 0),
        ]
    );
    let decoded = decode(&capture.bytes);
    assert!(decoded.waits.iter().all(|wait| *wait != 0));
    assert!(decoded.waits.contains(&u16::MAX));
    assert_eq!(
        decoded
            .waits
            .iter()
            .map(|wait| u64::from(*wait))
            .sum::<u64>(),
        970_200
    );
    let metadata = capture.metadata.nes.unwrap();
    assert_eq!(metadata.region, "ntsc");
    assert_eq!(metadata.clock_rounding, "nearest_integer_hz");
    assert_eq!(metadata.status_read_observation_count, 2);
    assert_eq!(capture.metadata.total_samples, 970_200);
    assert!(capture.metadata.limitations[2].contains("register projection"));
    let inspected = crate::vgm::inspect(
        &capture.bytes,
        crate::ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(inspected.samples, u64::from(capture.metadata.total_samples));
    assert_eq!(inspected.chips[0].name, "nes_apu");
    assert!(inspected.warnings.is_empty());
}

#[test]
fn pal_clock_is_qualified_while_dendy_is_explicitly_unavailable() {
    let mut pal_trace = trace(NesTraceRegion::Pal);
    pal_trace.events.push(register(PAL.0, 0x4011, 0x7f));
    pal_trace.end_cycle = 2 * PAL.0;
    let pal = encode_nes(&pal_trace, &AtomicBool::new(false))
        .unwrap()
        .capture
        .unwrap();
    assert_eq!(word(&pal.bytes, 0x84), 1_662_607);
    assert_eq!(pal.metadata.nes.unwrap().region, "pal");
    assert_eq!(word(&pal.bytes, 0x18), 2_822_400);
    assert_eq!(pal.metadata.total_samples, 2_822_400);
    assert_eq!(
        decode(&pal.bytes).writes,
        vec![(0, 0x4015, 0), (1_411_200, 0x4011, 0x7f)]
    );

    let dendy = encode_nes(&trace(NesTraceRegion::Dendy), &AtomicBool::new(false)).unwrap();
    assert!(dendy.capture.is_none());
    assert_eq!(dendy.unavailable[0].code, "dendy_region");
}

#[test]
fn dmc_and_duration_refusals_preserve_valid_native_trace_results() {
    let mut dmc_enabled = trace(NesTraceRegion::Ntsc);
    dmc_enabled.events.push(register(0, 0x4015, 0x10));
    let output = encode_nes(&dmc_enabled, &AtomicBool::new(false)).unwrap();
    assert!(output.capture.is_none());
    assert_eq!(output.unavailable[0].code, "dmc_enabled");

    for source in [
        AudioTraceSource::Unknown,
        AudioTraceSource::Unmapped,
        AudioTraceSource::WorkRam { offset: 3 },
    ] {
        let mut dmc_fetch = trace(NesTraceRegion::Ntsc);
        dmc_fetch.events.push(AudioTraceEvent {
            cycle: 1,
            pc: 0,
            instruction_source: AudioTraceSource::Unknown,
            write: NesTraceWrite::DmcFetch {
                address: 0xc000,
                value: 0xa5,
                source,
            },
        });
        let output = encode_nes(&dmc_fetch, &AtomicBool::new(false)).unwrap();
        assert!(output.capture.is_none());
        assert_eq!(output.unavailable[0].code, "dmc_fetch");
    }

    let dac = encode_nes(
        &{
            let mut source = trace(NesTraceRegion::Ntsc);
            source.events.push(register(0, 0x4011, 0xff));
            source
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(dac.capture.is_some());

    let mut overlong = trace(NesTraceRegion::Ntsc);
    overlong.end_cycle = 19_687_500 * 7_201;
    let output = encode_nes(&overlong, &AtomicBool::new(false)).unwrap();
    assert!(output.capture.is_none());
    assert_eq!(output.unavailable[0].code, "duration_limit");
}

#[test]
fn malformed_or_cancelled_traces_fail_before_projection() {
    assert!(encode_nes(&trace(NesTraceRegion::Ntsc), &AtomicBool::new(true)).is_err());

    let mut bad_reset = trace(NesTraceRegion::Ntsc);
    bad_reset.chip.initial_cpu_cycle = 0;
    assert!(encode_nes(&bad_reset, &AtomicBool::new(false)).is_err());
    bad_reset = trace(NesTraceRegion::Ntsc);
    bad_reset.chip.initial_cpu_cycle_odd = false;
    assert!(encode_nes(&bad_reset, &AtomicBool::new(false)).is_err());
    bad_reset = trace(NesTraceRegion::Ntsc);
    bad_reset.chip.initial_apu_frame_cycle = 0;
    assert!(encode_nes(&bad_reset, &AtomicBool::new(false)).is_err());
    bad_reset = trace(NesTraceRegion::Ntsc);
    bad_reset.chip.initial_half_rate_timer_clock = true;
    assert!(encode_nes(&bad_reset, &AtomicBool::new(false)).is_err());

    for region in [
        NesTraceRegion::Ntsc,
        NesTraceRegion::Pal,
        NesTraceRegion::Dendy,
    ] {
        let mut bad_clock = trace(region);
        bad_clock.chip.clock_hz_numerator += 1;
        assert!(encode_nes(&bad_clock, &AtomicBool::new(false)).is_err());
    }

    let mut bad_parity = trace(NesTraceRegion::Ntsc);
    bad_parity.events.push(event(
        0,
        NesTraceWrite::Register {
            address: 0x4011,
            value: 0,
            odd_cycle: false,
        },
    ));
    assert!(encode_nes(&bad_parity, &AtomicBool::new(false)).is_err());

    let mut bad_address = trace(NesTraceRegion::Ntsc);
    bad_address.events.push(register(0, 0x4014, 0));
    assert!(encode_nes(&bad_address, &AtomicBool::new(false)).is_err());

    let mut bad_pc = trace(NesTraceRegion::Ntsc);
    let mut out_of_range = register(0, 0x4011, 0);
    out_of_range.pc = u32::from(u16::MAX) + 1;
    bad_pc.events.push(out_of_range);
    assert!(encode_nes(&bad_pc, &AtomicBool::new(false)).is_err());

    let mut at_end = trace(NesTraceRegion::Ntsc);
    at_end.events.push(register(at_end.end_cycle, 0x4011, 0));
    assert!(encode_nes(&at_end, &AtomicBool::new(false)).is_err());

    let mut wrong_timing = trace(NesTraceRegion::Ntsc);
    wrong_timing.timing = AudioTraceTiming::InstructionBoundary;
    assert!(encode_nes(&wrong_timing, &AtomicBool::new(false)).is_err());

    let mut unordered = trace(NesTraceRegion::Ntsc);
    unordered.events = vec![register(2, 0x4011, 0), register(1, 0x4011, 0)];
    assert!(encode_nes(&unordered, &AtomicBool::new(false)).is_err());

    let mut bad_status = trace(NesTraceRegion::Ntsc);
    bad_status.events.push(event(
        0,
        NesTraceWrite::StatusRead {
            value: 0x20,
            origin: NesTraceOrigin::Cpu,
        },
    ));
    assert!(encode_nes(&bad_status, &AtomicBool::new(false)).is_err());

    let mut bad_fetch = trace(NesTraceRegion::Ntsc);
    bad_fetch.events.push(AudioTraceEvent {
        cycle: 0,
        pc: 1,
        instruction_source: AudioTraceSource::Unknown,
        write: NesTraceWrite::DmcFetch {
            address: 0x7fff,
            value: 0,
            source: AudioTraceSource::Unknown,
        },
    });
    assert!(encode_nes(&bad_fetch, &AtomicBool::new(false)).is_err());
}
