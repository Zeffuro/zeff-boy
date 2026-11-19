use std::sync::atomic::AtomicBool;

use zeff_emu_common::audio_trace::*;

use super::{encode_md, validate_md_trace};

fn trace(region: MdTraceRegion) -> MdAudioTrace {
    let master_hz = match region {
        MdTraceRegion::NtscUs | MdTraceRegion::NtscJapan => 53_693_175,
        MdTraceRegion::Pal => 53_203_424,
    };
    MdAudioTrace {
        generation: 9,
        cycle_hz: master_hz as u32,
        cycle_hz_denominator: 1,
        chip: MdTraceChip {
            master_hz,
            region,
            psg_divisor: 240,
            opn2_divisor: 7,
            reset: MdTraceReset::ZeffPowerOnV1,
        },
        timing: AudioTraceTiming::BusServiceBoundary,
        start: AudioTraceStart::Reset,
        end_cycle: master_hz,
        events: Vec::new(),
        dropped_events: 0,
        invalidated: None,
    }
}

fn origin() -> MdTraceOrigin {
    MdTraceOrigin {
        writer: MdTraceWriter::M68000,
        boundary_pc: 0x1234,
        source: MdTraceSource::EffectiveRom { offset: 0x1234 },
    }
}

fn event(cycle: u64, write: MdTraceWrite) -> AudioTraceEvent<MdTraceWrite> {
    AudioTraceEvent {
        cycle,
        pc: origin().boundary_pc,
        instruction_source: AudioTraceSource::Unknown,
        write,
    }
}

fn port(cycle: u64, port: u8, value: u8) -> AudioTraceEvent<MdTraceWrite> {
    event(
        cycle,
        MdTraceWrite::Opn2Port {
            origin: origin(),
            port,
            value,
        },
    )
}

fn psg(cycle: u64, value: u8) -> AudioTraceEvent<MdTraceWrite> {
    event(
        cycle,
        MdTraceWrite::Psg {
            origin: origin(),
            value,
        },
    )
}

fn capture(trace: &MdAudioTrace) -> super::VgmCapture {
    let export = encode_md(trace, &AtomicBool::new(false)).unwrap();
    assert!(export.unavailable.is_empty());
    export.capture.unwrap()
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[derive(Debug, PartialEq, Eq)]
struct Write {
    sample: u64,
    bytes: Vec<u8>,
}

fn decoded(bytes: &[u8]) -> (Vec<Write>, u64) {
    let mut offset = 0x100;
    let mut sample = 0;
    let mut writes = Vec::new();
    loop {
        let len = match bytes[offset] {
            0x50 => 2,
            0x52 | 0x53 => 3,
            0x61 => {
                let wait = u16::from_le_bytes(bytes[offset + 1..offset + 3].try_into().unwrap());
                assert_ne!(wait, 0);
                sample += u64::from(wait);
                offset += 3;
                continue;
            }
            0x66 => {
                assert_eq!(offset + 1, bytes.len());
                break;
            }
            opcode => panic!("unexpected VGM opcode {opcode:02x}"),
        };
        writes.push(Write {
            sample,
            bytes: bytes[offset..offset + len].to_vec(),
        });
        offset += len;
    }
    (writes, sample)
}

fn guest_commands(capture: &super::VgmCapture) -> Vec<Vec<u8>> {
    decoded(&capture.bytes)
        .0
        .into_iter()
        .skip(217)
        .map(|row| row.bytes)
        .collect()
}

#[test]
fn regional_clocks_absolute_floor_timing_and_end_events() {
    for (region, psg_clock, opn2_clock) in [
        (MdTraceRegion::NtscUs, 3_579_545, 7_670_454),
        (MdTraceRegion::NtscJapan, 3_579_545, 7_670_454),
        (MdTraceRegion::Pal, 3_546_895, 7_600_489),
    ] {
        let mut trace = trace(region);
        trace.end_cycle += 1234;
        let first = trace.chip.master_hz / 4 + 17;
        trace.events = vec![
            psg(first, 0x91),
            port(first, 0, 0x2a),
            port(first, 3, 0x7f),
            psg(trace.end_cycle, 0x9f),
        ];
        let capture = capture(&trace);
        assert_eq!(word(&capture.bytes, 0x0c), psg_clock);
        assert_eq!(word(&capture.bytes, 0x2c), opn2_clock);
        assert_eq!(&capture.bytes[0x28..0x2c], &[9, 0, 16, 4]);
        assert_eq!(word(&capture.bytes, 4) as usize + 4, capture.bytes.len());
        let (writes, samples) = decoded(&capture.bytes);
        let floor = |cycle| (u128::from(cycle) * 44_100 / u128::from(trace.chip.master_hz)) as u64;
        assert_eq!(writes[217].sample, floor(first));
        assert_eq!(writes[218].sample, floor(first));
        assert_eq!(writes[219].sample, floor(trace.end_cycle));
        assert_eq!(samples, floor(trace.end_cycle));
        assert_eq!(word(&capture.bytes, 0x18), samples as u32);
        assert_eq!(capture.metadata.guest_write_count, 3);
        let md = capture.metadata.md.unwrap();
        assert_eq!(md.psg_clock_hz_numerator, trace.chip.master_hz);
        assert_eq!(md.psg_clock_hz_denominator, 15);
        assert_eq!(md.opn2_clock_hz_denominator, 7);
        assert_eq!(md.clock_rounding, "nearest_integer_hz");
        assert_eq!(
            (
                md.native_event_count,
                md.psg_write_count,
                md.opn2_address_write_count,
                md.opn2_register_write_count
            ),
            (4, 2, 1, 1)
        );
    }
}

#[test]
fn preamble_initializes_ordinary_registers_without_synthetic_keys() {
    let capture = capture(&trace(MdTraceRegion::NtscUs));
    let (writes, _) = decoded(&capture.bytes);
    assert_eq!(capture.metadata.preamble_write_count, 217);
    assert_eq!(writes.len(), 217);
    assert!(writes.iter().all(|write| write.sample == 0));
    let bytes: Vec<_> = writes[..12].iter().map(|write| write.bytes[1]).collect();
    assert_eq!(
        bytes,
        [
            0x80, 0, 0xa0, 0, 0xc0, 0, 0x9f, 0xbf, 0xdf, 0xff, 0xe0, 0x80
        ]
    );
    assert_eq!(writes[12].bytes, [0x52, 0x22, 0]);
    assert_eq!(writes[17].bytes, [0x52, 0x2a, 0x80]);
    for bank in [0x52, 0x53] {
        let operators: Vec<_> = writes
            .iter()
            .filter(|write| write.bytes[0] == bank && (0x30..=0x9f).contains(&write.bytes[1]))
            .collect();
        assert_eq!(operators.len(), 84);
        assert!(
            operators
                .iter()
                .all(|write| write.bytes[1] & 3 != 3 && write.bytes[2] == 0)
        );
        for channel in 0..3 {
            for pair in [
                [bank, 0xa4 + channel, 0],
                [bank, 0xa0 + channel, 0],
                [bank, 0xb0 + channel, 0],
                [bank, 0xb4 + channel, 0xc0],
            ] {
                assert_eq!(writes.iter().filter(|write| write.bytes == pair).count(), 1);
            }
        }
    }
    assert!(
        !writes
            .iter()
            .any(|write| write.bytes[0] != 0x50 && write.bytes[1] == 0x28)
    );
    for (start, bank) in [(103, 0x52), (199, 0x53)] {
        let expected: Vec<_> = (0..3)
            .flat_map(|channel| {
                [
                    vec![bank, 0xa4 + channel, 0],
                    vec![bank, 0xa0 + channel, 0],
                    vec![bank, 0xb0 + channel, 0],
                    vec![bank, 0xb4 + channel, 0xc0],
                ]
            })
            .collect();
        let actual: Vec<_> = writes[start..start + 12]
            .iter()
            .map(|write| write.bytes.clone())
            .collect();
        assert_eq!(actual, expected);
    }
    assert_eq!(
        writes[211..]
            .iter()
            .map(|write| write.bytes.clone())
            .collect::<Vec<_>>(),
        [
            vec![0x52, 0xac, 0],
            vec![0x52, 0xa8, 0],
            vec![0x52, 0xad, 0],
            vec![0x52, 0xa9, 0],
            vec![0x52, 0xae, 0],
            vec![0x52, 0xaa, 0],
        ]
    );
}

#[test]
fn shared_latch_crosses_data_ports_and_preserves_frequency_and_dac_order() {
    let mut trace = trace(MdTraceRegion::NtscUs);
    trace.events = vec![
        port(56, 0, 0x2a),
        port(56, 3, 0x91),
        port(56, 2, 0xb6),
        port(56, 1, 0xc0),
        port(57, 0, 0xa4),
        port(57, 3, 0x12),
        port(58, 2, 0xa5),
        port(58, 1, 0x23),
        port(59, 0, 0xa0),
        psg(59, 0x80),
        port(59, 3, 0x34),
        port(60, 2, 0xa1),
        port(60, 1, 0x45),
        port(61, 0, 0x2a),
        port(61, 3, 0x80),
        port(61, 1, 0x80),
    ];
    assert_eq!(
        guest_commands(&capture(&trace)),
        vec![
            vec![0x52, 0x2a, 0x91],
            vec![0x53, 0xb6, 0xc0],
            vec![0x52, 0xa4, 0x12],
            vec![0x53, 0xa5, 0x23],
            vec![0x50, 0x80],
            vec![0x52, 0xa0, 0x34],
            vec![0x53, 0xa1, 0x45],
            vec![0x52, 0x2a, 0x80],
            vec![0x52, 0x2a, 0x80],
        ]
    );
}

#[test]
fn address_only_writes_are_accounted_without_commands_or_native_mutation() {
    let mut trace = trace(MdTraceRegion::NtscUs);
    trace.events = vec![psg(56, 0x90), port(trace.end_cycle, 2, 0xff)];
    let before = trace.clone();
    let capture = capture(&trace);
    assert_eq!(trace, before);
    assert_eq!(guest_commands(&capture), [vec![0x50, 0x90]]);
    let md = capture.metadata.md.unwrap();
    assert_eq!(
        (
            md.native_event_count,
            md.psg_write_count,
            md.opn2_address_write_count,
            md.opn2_register_write_count
        ),
        (2, 1, 1, 0)
    );
}

#[test]
fn reset_address_zero_and_unsupported_destinations_refuse_the_complete_projection() {
    let mut trace = trace(MdTraceRegion::NtscUs);
    trace.events = vec![
        psg(56, 0x90),
        port(57, 1, 0xff),
        port(58, 2, 0x22),
        port(59, 1, 0),
        port(60, 0, 0x33),
        port(61, 3, 0),
        port(62, 0, 0x21),
        port(63, 1, 0),
        event(64, MdTraceWrite::Opn2Reset { origin: origin() }),
        event(65, MdTraceWrite::Opn2Reset { origin: origin() }),
    ];
    trace.end_cycle = trace.chip.master_hz * 7201;
    let export = encode_md(&trace, &AtomicBool::new(false)).unwrap();
    assert!(export.capture.is_none());
    assert_eq!(
        export
            .unavailable
            .iter()
            .map(|row| (row.code, row.first_event_index))
            .collect::<Vec<_>>(),
        [
            ("opn2_unmapped_register", Some(1)),
            ("opn2_reset", Some(8)),
            ("duration_limit", None),
        ]
    );
}

#[test]
fn every_register_destination_matches_the_admitted_native_whitelist() {
    let globals = [0x22, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2a, 0x2b];
    let channel = [
        0xa0, 0xa1, 0xa2, 0xa4, 0xa5, 0xa6, 0xb0, 0xb1, 0xb2, 0xb4, 0xb5, 0xb6,
    ];
    let special = [0xa8, 0xa9, 0xaa, 0xac, 0xad, 0xae];
    for bank in 0..=1 {
        for address in 0u8..=255 {
            let expected = (bank == 0
                && (globals.contains(&address) || special.contains(&address)))
                || channel.contains(&address)
                || ((0x30..=0x9f).contains(&address) && ![3, 7, 11, 15].contains(&(address & 15)));
            let mut trace = trace(MdTraceRegion::NtscUs);
            trace.events = vec![port(56, bank * 2, address), port(57, 1, 0xff)];
            let result = encode_md(&trace, &AtomicBool::new(false)).unwrap();
            assert_eq!(
                result.capture.is_some(),
                expected,
                "bank={bank} address={address:02x}"
            );
            if let Some(capture) = result.capture {
                assert_eq!(guest_commands(&capture), [vec![0x52 + bank, address, 0xff]]);
            } else {
                assert_eq!(result.unavailable[0].first_event_index, Some(1));
            }
        }
    }
}

#[test]
fn reset_before_data_refuses_and_resets_the_native_mapping_latch() {
    let mut trace = trace(MdTraceRegion::NtscUs);
    trace.events = vec![
        port(56, 0, 0x2a),
        event(57, MdTraceWrite::Opn2Reset { origin: origin() }),
        port(58, 1, 0x80),
    ];
    let export = encode_md(&trace, &AtomicBool::new(false)).unwrap();
    assert!(export.capture.is_none());
    assert_eq!(
        export
            .unavailable
            .iter()
            .map(|row| (row.code, row.first_event_index))
            .collect::<Vec<_>>(),
        [("opn2_reset", Some(1)), ("opn2_unmapped_register", Some(2)),]
    );
}

#[test]
fn malformed_descriptors_events_and_generic_origins_are_errors() {
    type TraceMutation = fn(&mut MdAudioTrace);
    let mutations: &[(&str, TraceMutation)] = &[
        ("master", |trace| trace.chip.master_hz += 1),
        ("region", |trace| trace.chip.region = MdTraceRegion::Pal),
        ("cycle clock", |trace| trace.cycle_hz -= 1),
        ("denominator", |trace| trace.cycle_hz_denominator = 2),
        ("timing", |trace| {
            trace.timing = AudioTraceTiming::InstructionBoundary
        }),
        ("PSG divisor", |trace| trace.chip.psg_divisor = 15),
        ("OPN2 divisor", |trace| trace.chip.opn2_divisor = 6),
        ("dropped", |trace| trace.dropped_events = 1),
        ("invalidated", |trace| {
            trace.invalidated = Some(AudioTraceInvalidation::Reset)
        }),
        ("past end", |trace| {
            trace.events[0].cycle = trace.end_cycle + 1
        }),
        ("backward", |trace| trace.events.push(psg(1, 0))),
        ("port", |trace| trace.events[0] = port(56, 4, 0)),
        ("PC mismatch", |trace| trace.events[0].pc += 1),
        ("generic source", |trace| {
            trace.events[0].instruction_source = AudioTraceSource::WorkRam { offset: 0x1234 }
        }),
    ];
    for (name, mutate) in mutations {
        let mut trace = trace(MdTraceRegion::NtscUs);
        trace.events.push(psg(56, 0x90));
        mutate(&mut trace);
        assert!(
            encode_md(&trace, &AtomicBool::new(false)).is_err(),
            "{name}"
        );
    }
}

fn with_origin(origin: MdTraceOrigin) -> MdAudioTrace {
    let mut trace = trace(MdTraceRegion::NtscUs);
    trace.events.push(AudioTraceEvent {
        cycle: 56,
        pc: origin.boundary_pc,
        instruction_source: AudioTraceSource::Unknown,
        write: MdTraceWrite::Psg {
            origin,
            value: 0x90,
        },
    });
    trace
}

#[test]
fn architectural_m68000_pc_and_typed_ram_mirrors_remain_valid() {
    for origin in [
        MdTraceOrigin {
            boundary_pc: 0xab00_1234,
            ..origin()
        },
        MdTraceOrigin {
            boundary_pc: 0xabff_1234,
            source: MdTraceSource::MainRam { offset: 0x1234 },
            ..origin()
        },
        MdTraceOrigin {
            boundary_pc: 0x2000,
            writer: MdTraceWriter::Z80,
            source: MdTraceSource::SoundRam { offset: 0 },
        },
        MdTraceOrigin {
            boundary_pc: 0x9234,
            writer: MdTraceWriter::Z80,
            source: MdTraceSource::EffectiveRom { offset: 0x1_1234 },
        },
        MdTraceOrigin {
            boundary_pc: 0x9234,
            writer: MdTraceWriter::Z80,
            source: MdTraceSource::MainRam { offset: 0x9234 },
        },
        MdTraceOrigin {
            boundary_pc: 0x4321,
            writer: MdTraceWriter::Z80,
            source: MdTraceSource::Unknown,
        },
        MdTraceOrigin {
            boundary_pc: 0x20_0000,
            source: MdTraceSource::Unknown,
            ..origin()
        },
    ] {
        validate_md_trace(&with_origin(origin), &AtomicBool::new(false)).unwrap();
        assert_eq!(capture(&with_origin(origin)).metadata.guest_write_count, 1);
    }
}

#[test]
fn malformed_typed_origins_are_errors() {
    for origin in [
        MdTraceOrigin {
            source: MdTraceSource::EffectiveRom { offset: 0x40_0000 },
            ..origin()
        },
        MdTraceOrigin {
            source: MdTraceSource::EffectiveRom { offset: 0x1235 },
            ..origin()
        },
        MdTraceOrigin {
            source: MdTraceSource::MainRam { offset: 0x1_0000 },
            ..origin()
        },
        MdTraceOrigin {
            source: MdTraceSource::MainRam { offset: 0x1234 },
            ..origin()
        },
        MdTraceOrigin {
            source: MdTraceSource::SoundRam { offset: 0x1234 },
            ..origin()
        },
        MdTraceOrigin {
            writer: MdTraceWriter::Z80,
            boundary_pc: 0x1_1234,
            source: MdTraceSource::Unknown,
        },
        MdTraceOrigin {
            writer: MdTraceWriter::Z80,
            boundary_pc: 0x1234,
            source: MdTraceSource::SoundRam { offset: 0x2000 },
        },
        MdTraceOrigin {
            writer: MdTraceWriter::Z80,
            boundary_pc: 0x2000,
            source: MdTraceSource::SoundRam { offset: 1 },
        },
        MdTraceOrigin {
            writer: MdTraceWriter::Z80,
            boundary_pc: 0x8000,
            source: MdTraceSource::SoundRam { offset: 0 },
        },
        MdTraceOrigin {
            writer: MdTraceWriter::Z80,
            boundary_pc: 0x4000,
            source: MdTraceSource::EffectiveRom { offset: 0x4000 },
        },
        MdTraceOrigin {
            writer: MdTraceWriter::Z80,
            boundary_pc: 0x9234,
            source: MdTraceSource::MainRam { offset: 0x1235 },
        },
    ] {
        assert!(
            encode_md(&with_origin(origin), &AtomicBool::new(false)).is_err(),
            "{origin:?}"
        );
    }
}

#[test]
fn cancellation_is_an_error_even_for_a_refused_trace() {
    let mut trace = trace(MdTraceRegion::Pal);
    trace
        .events
        .push(event(56, MdTraceWrite::Opn2Reset { origin: origin() }));
    assert!(encode_md(&trace, &AtomicBool::new(true)).is_err());
    assert!(validate_md_trace(&trace, &AtomicBool::new(true)).is_err());
}

#[test]
fn native_event_and_duration_bounds_fit_stream_command_and_size_limits() {
    let waits =
        MAX_AUDIO_TRACE_EVENTS + (crate::vgm::MAX_SAMPLES.div_ceil(u64::from(u16::MAX))) as usize;
    let commands = 217 + MAX_AUDIO_TRACE_EVENTS + waits + 1;
    let bytes = 0x100 + 639 + MAX_AUDIO_TRACE_EVENTS * 4 + waits * 3 + 1;
    assert!(commands <= crate::vgm::MAX_COMMANDS as usize);
    assert!(bytes <= crate::vgm::MAX_ROM_BYTES);
}
