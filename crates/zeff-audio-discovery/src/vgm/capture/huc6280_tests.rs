use std::sync::atomic::AtomicBool;

use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, AudioTraceStart, AudioTraceTiming,
    Huc6280AudioTrace, Huc6280ResetState, Huc6280TraceChip, Huc6280TraceRevision,
    Huc6280TraceWrite, MAX_AUDIO_TRACE_EVENTS,
};

use super::{VgmCapture, VgmCaptureSource, encode_huc6280};

fn trace() -> Huc6280AudioTrace {
    Huc6280AudioTrace {
        generation: 11,
        cycle_hz: 236_250_000,
        cycle_hz_denominator: 11,
        chip: Huc6280TraceChip {
            clock_hz_numerator: 315_000_000,
            clock_hz_denominator: 88,
            master_clock_divisor: 6,
            internal_master_clock_divisor: 3,
            revision: Huc6280TraceRevision::HuC6280,
            reset: Huc6280ResetState::default(),
        },
        timing: AudioTraceTiming::MemoryWriteCompletion,
        start: AudioTraceStart::Reset,
        end_cycle: 0,
        events: Vec::new(),
        dropped_events: 0,
        invalidated: None,
    }
}

fn event(cycle: u64, address: u32, value: u8) -> AudioTraceEvent<Huc6280TraceWrite> {
    AudioTraceEvent {
        cycle,
        pc: 0xe040,
        instruction_source: AudioTraceSource::CartridgeRom {
            offset: 0x2040,
            bit_reversed: true,
        },
        write: Huc6280TraceWrite {
            physical_address: address,
            register: (address & 15) as u8,
            value,
        },
    }
}

fn encode(trace: &Huc6280AudioTrace) -> VgmCapture {
    trace.encode_vgm(&AtomicBool::new(false)).unwrap()
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

struct Decoded {
    writes: Vec<(u64, u8, u8)>,
    waits: Vec<u16>,
    samples: u64,
}

fn decode(capture: &VgmCapture) -> Decoded {
    let bytes = &capture.bytes;
    assert_eq!(&bytes[..4], b"Vgm ");
    assert_eq!(word(bytes, 4) as usize + 4, bytes.len());
    assert_eq!(word(bytes, 8), 0x171);
    assert_eq!(word(bytes, 0x0c), 0);
    assert_eq!(word(bytes, 0x14), 0);
    assert_eq!(word(bytes, 0x1c), 0);
    assert_eq!(word(bytes, 0x20), 0);
    assert_eq!(word(bytes, 0x34), 0xcc);
    let mut offset = 0x34 + word(bytes, 0x34) as usize;
    let mut decoded = Decoded {
        writes: Vec::new(),
        waits: Vec::new(),
        samples: 0,
    };
    loop {
        match bytes[offset] {
            0xb9 => {
                decoded
                    .writes
                    .push((decoded.samples, bytes[offset + 1], bytes[offset + 2]));
                offset += 3;
            }
            0x61 => {
                let wait = u16::from_le_bytes(bytes[offset + 1..offset + 3].try_into().unwrap());
                assert_ne!(wait, 0);
                decoded.waits.push(wait);
                decoded.samples += u64::from(wait);
                offset += 3;
            }
            0x66 => {
                assert_eq!(offset + 1, bytes.len());
                break;
            }
            opcode => panic!("unexpected VGM opcode: {opcode:02x}"),
        }
    }
    assert_eq!(word(bytes, 0x18), decoded.samples as u32);
    assert_eq!(capture.metadata.total_samples, decoded.samples as u32);
    assert_eq!(
        capture.metadata.command_count as usize,
        decoded.writes.len() + decoded.waits.len() + 1
    );
    decoded
}

#[test]
fn reset_preamble_restores_registers_wave_ram_dda_and_selection() {
    let capture = encode(&trace());
    let decoded = decode(&capture);
    assert_eq!(capture.metadata.preamble_write_count, 241);
    assert_eq!(capture.metadata.guest_write_count, 0);
    assert_eq!(decoded.writes.len(), 241);
    assert_eq!(decoded.samples, 0);
    let mut selected = 7usize;
    let mut global = [0xff; 3];
    let mut controls = [0xc7; 6];
    let mut frequencies = [0xfff; 6];
    let mut balances = [0xff; 6];
    let mut noises = [0x9f; 2];
    let mut waveforms = [[0x1f; 32]; 6];
    let mut cursors = [17usize; 6];
    let mut dda = [0x1f; 6];
    let mut wave_writes = [0; 6];
    for &(time, register, value) in &decoded.writes {
        assert_eq!(time, 0);
        match register {
            0 => selected = usize::from(value & 7),
            1 => global[0] = value,
            8 => global[1] = value,
            9 => global[2] = value,
            2 => frequencies[selected] = frequencies[selected] & 0xf00 | u16::from(value),
            3 => frequencies[selected] = frequencies[selected] & 0xff | u16::from(value & 15) << 8,
            4 => {
                if controls[selected] & 0x40 != 0 || value & 0x40 != 0 {
                    cursors[selected] = 0;
                }
                controls[selected] = value;
            }
            5 => balances[selected] = value,
            6 if controls[selected] & 0x40 != 0 => dda[selected] = value & 31,
            6 => {
                assert_eq!(controls[selected] & 0x80, 0);
                waveforms[selected][cursors[selected]] = value & 31;
                cursors[selected] = (cursors[selected] + 1) & 31;
                wave_writes[selected] += 1;
            }
            7 => noises[selected - 4] = value,
            register => panic!("unexpected preamble register {register}"),
        }
    }
    assert_eq!(selected, 0);
    assert_eq!(global, [0; 3]);
    assert_eq!(controls, [0; 6]);
    assert_eq!(frequencies, [0; 6]);
    assert_eq!(balances, [0; 6]);
    assert_eq!(noises, [0; 2]);
    assert_eq!(waveforms, [[0; 32]; 6]);
    assert_eq!(cursors, [0; 6]);
    assert_eq!(dda, [0; 6]);
    assert_eq!(wave_writes, [32; 6]);
}

#[test]
fn rational_timing_raw_alias_writes_and_terminal_wait_are_preserved() {
    let mut source = trace();
    source.end_cycle = 236_250_007;
    for (index, cycle) in [0, 0, 1, 487, 488, 1_432_000, 78_750_000, 236_250_006]
        .into_iter()
        .enumerate()
    {
        for register in 0..16 {
            source.events.push(event(
                cycle,
                0x1febf0 + register,
                (index * 31 + register as usize) as u8,
            ));
        }
    }
    let capture = encode(&source);
    let decoded = decode(&capture);
    let expected: Vec<_> = source
        .events
        .iter()
        .map(|event| {
            let time = u128::from(event.cycle) * 11 * 44_100 / 236_250_000;
            (time as u64, event.write.register, event.write.value)
        })
        .collect();
    assert_eq!(&decoded.writes[241..], expected);
    assert_eq!(decoded.samples, 485_100);
    assert!(decoded.waits.contains(&u16::MAX));
    assert_eq!(capture.metadata.timing, "memory_write_completion");
    assert_eq!(capture.metadata.cycle_hz_denominator, 11);
    assert_eq!(capture.metadata.sn76489_flags, None);
    assert_eq!(word(&capture.bytes, 0xa4), 3_579_545);
    let metadata = capture.metadata.huc6280.as_ref().unwrap();
    assert_eq!(
        (metadata.clock_hz_numerator, metadata.clock_hz_denominator),
        (315_000_000, 88)
    );
    assert_eq!(metadata.vgm_clock_hz, 3_579_545);
    assert_eq!(metadata.revision, "huc6280");
    let json = serde_json::to_value(&capture.metadata).unwrap();
    assert!(json.get("sn76489_flags").is_none());
    assert_eq!(json["cycle_hz_denominator"], 11);
    let log = crate::vgm::inspect(
        &capture.bytes,
        crate::ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(log.warnings.is_empty());
    assert_eq!(log.chips.len(), 1);
    assert_eq!(log.chips[0].name, "huc6280");
    assert_eq!(log.chips[0].raw_clock, 3_579_545);
    assert_eq!(log.samples, decoded.samples);
    source.chip.revision = Huc6280TraceRevision::HuC6280A;
    assert_eq!(
        encode(&source).metadata.huc6280.unwrap().revision,
        "huc6280a"
    );
}

#[test]
fn equivalent_clock_fractions_and_half_up_rounding_are_explicit() {
    let mut source = trace();
    source.cycle_hz = 21;
    source.cycle_hz_denominator = 1;
    source.chip.clock_hz_numerator = 7;
    source.chip.clock_hz_denominator = 2;
    source.end_cycle = 21;
    let capture = encode(&source);
    assert_eq!(word(&capture.bytes, 0xa4), 4);
    assert_eq!(decode(&capture).samples, 44_100);
    source.cycle_hz *= 3;
    source.cycle_hz_denominator *= 3;
    source.chip.clock_hz_numerator *= 9;
    source.chip.clock_hz_denominator *= 9;
    assert_eq!(encode(&source).bytes, capture.bytes);
}

#[test]
fn malformed_clocks_addresses_states_and_incomplete_traces_reject() {
    for invalid in 0..19 {
        let mut source = trace();
        source.events.push(event(0, 0x1fe800, 0));
        match invalid {
            0 => source.cycle_hz = 0,
            1 => source.cycle_hz_denominator = 0,
            2 => source.chip.clock_hz_numerator = 0,
            3 => source.chip.clock_hz_denominator = 0,
            4 => source.chip.clock_hz_numerator += 1,
            5 => source.chip.master_clock_divisor = 3,
            6 => source.chip.internal_master_clock_divisor = 6,
            7 => source.chip.reset.channels[3].waveform[11] = 1,
            8 => source.chip.reset.channels[5].noise_seed = 2,
            9 => source.chip.reset.gain_scan_active = true,
            10 => source.events[0].write.physical_address = 0x1fe7ff,
            11 => source.events[0].write.physical_address = 0x1fec00,
            12 => source.events[0].write.register = 16,
            13 => source.events[0].write.register = 1,
            14 => source.events[0].cycle = 1,
            15 => source.dropped_events = 1,
            16 => source.invalidated = Some(AudioTraceInvalidation::StateRestore),
            17 => {
                source.end_cycle = 2;
                source.events = vec![event(2, 0x1fe800, 0), event(1, 0x1fe800, 0)];
            }
            18 => {
                source.cycle_hz = 1;
                source.cycle_hz_denominator = 1;
                source.chip.clock_hz_numerator = 1;
                source.chip.clock_hz_denominator = 6;
            }
            _ => unreachable!(),
        }
        assert!(
            encode_huc6280(&source, &AtomicBool::new(false)).is_err(),
            "invalid case {invalid}"
        );
    }
    assert!(encode_huc6280(&trace(), &AtomicBool::new(true)).is_err());
}

#[test]
fn exact_event_and_duration_limits_are_bounded() {
    let mut source = trace();
    source.events = vec![event(0, 0x1fe80f, 0xff); MAX_AUDIO_TRACE_EVENTS];
    assert_eq!(
        decode(&encode(&source)).writes.len(),
        MAX_AUDIO_TRACE_EVENTS + 241
    );
    source.events.push(event(0, 0x1fe800, 0));
    assert!(encode_huc6280(&source, &AtomicBool::new(false)).is_err());
    source.events.clear();
    source.cycle_hz = 44_100;
    source.cycle_hz_denominator = 1;
    source.chip.clock_hz_numerator = 7_350;
    source.chip.clock_hz_denominator = 1;
    source.end_cycle = crate::vgm::MAX_SAMPLES;
    assert_eq!(decode(&encode(&source)).samples, crate::vgm::MAX_SAMPLES);
    source.end_cycle += 1;
    assert!(encode_huc6280(&source, &AtomicBool::new(false)).is_err());
    source.end_cycle = u64::MAX;
    assert!(encode_huc6280(&source, &AtomicBool::new(false)).is_err());
}
