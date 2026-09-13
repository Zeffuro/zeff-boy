use std::sync::atomic::AtomicBool;

use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, Sn76489ResetState,
    Sn76489Tone2NoiseClock,
};

use super::*;
use crate::vgm::{MAX_SAMPLES, TICKS_PER_SECOND};
use zeff_emu_common::audio_trace::{
    AudioTraceStart, AudioTraceTiming, AudioTraceWrite, Sn76489TraceChip, Sn76489ZeroPeriod,
};

fn encode(trace: &AudioTrace) -> Result<VgmCapture> {
    super::encode_sn76489(trace, &AtomicBool::new(false))
}

fn chip(stereo: bool) -> Sn76489TraceChip {
    Sn76489TraceChip {
        clock_hz: 3_584_160,
        feedback_mask: 0x0009,
        shift_register_width: 16,
        zero_period: Sn76489ZeroPeriod::ConstantHigh,
        period_one_constant_high: true,
        tone_counter_clock_divider: 16,
        noise_tone2_clock: Sn76489Tone2NoiseClock::HalfPeriod,
        noise_output_high_when_lfsr_bit_zero: true,
        stereo,
        reset: Sn76489ResetState {
            tone_periods: [0, 0, 0],
            volumes: [15, 15, 15, 15],
            noise_control: 0,
            stereo_control: 0xff,
            latched_register: 0,
            noise_lfsr: 0x8000,
            tone_output_high: [true; 3],
            tone_clocks_remaining: [16; 3],
            noise_clocks_remaining: 512,
        },
    }
}

fn trace(events: Vec<AudioTraceEvent>, end_cycle: u64) -> AudioTrace {
    AudioTrace {
        generation: 7,
        cycle_hz: TICKS_PER_SECOND,
        cycle_hz_denominator: 1,
        chip: chip(true),
        timing: AudioTraceTiming::InstructionBoundary,
        start: AudioTraceStart::Reset,
        end_cycle,
        events,
        dropped_events: 0,
        invalidated: None,
    }
}

fn sn(cycle: u64, value: u8) -> AudioTraceEvent {
    AudioTraceEvent {
        cycle,
        pc: 0x1234,
        instruction_source: AudioTraceSource::CartridgeRom {
            offset: 0x4000,
            bit_reversed: false,
        },
        write: AudioTraceWrite::Sn76489 { port: 0x7f, value },
    }
}

#[test]
fn ordered_writes_and_terminal_wait_are_preserved_and_parse_cleanly() {
    let capture = encode(&trace(
        vec![
            sn(0, 0x90),
            AudioTraceEvent {
                cycle: 0,
                pc: 0x1235,
                instruction_source: AudioTraceSource::Unknown,
                write: AudioTraceWrite::GameGearStereo {
                    port: 0x06,
                    value: 0x10,
                },
            },
            sn(5, 0x80),
        ],
        7,
    ))
    .unwrap();

    assert_eq!(&capture.bytes[..4], b"Vgm ");
    assert_eq!(
        u32::from_le_bytes(capture.bytes[8..12].try_into().unwrap()),
        VGM_VERSION
    );
    assert_eq!(
        u32::from_le_bytes(capture.bytes[0x34..0x38].try_into().unwrap()),
        0xcc
    );
    assert_eq!(capture.metadata.preamble_write_count, 13);
    assert_eq!(capture.metadata.guest_write_count, 3);
    assert_eq!(capture.metadata.total_samples, 7);
    assert_eq!(
        &capture.bytes[HEADER_LEN + 26..],
        &[
            0x50, 0x90, 0x4f, 0x10, 0x61, 5, 0, 0x50, 0x80, 0x61, 2, 0, 0x66
        ]
    );
    let log = crate::vgm::inspect(
        &capture.bytes,
        crate::ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(log.samples, 7);
    assert_eq!(log.declared_samples, 7);
    assert_eq!(log.chips[0].raw_clock, 3_584_160);
    assert!(log.warnings.is_empty());
}

#[test]
fn header_preserves_actual_sega_and_coleco_chip_descriptors() {
    let sega = encode(&trace(Vec::new(), 0)).unwrap();
    assert_eq!(sega.metadata.sn76489_flags, Some(0x00));
    assert_eq!(
        u16::from_le_bytes(sega.bytes[0x28..0x2a].try_into().unwrap()),
        0x0009
    );
    assert_eq!(sega.bytes[0x2a], 16);

    let mut coleco_trace = trace(Vec::new(), 0);
    coleco_trace.chip.clock_hz = 3_579_545;
    coleco_trace.chip.feedback_mask = 0x0003;
    coleco_trace.chip.shift_register_width = 15;
    coleco_trace.chip.zero_period = Sn76489ZeroPeriod::Period1024;
    coleco_trace.chip.noise_tone2_clock = Sn76489Tone2NoiseClock::RisingEdge;
    coleco_trace.chip.noise_output_high_when_lfsr_bit_zero = false;
    coleco_trace.chip.stereo = false;
    coleco_trace.chip.reset.noise_lfsr = 0x4000;
    let coleco = encode(&coleco_trace).unwrap();
    assert_eq!(coleco.metadata.sn76489_flags, Some(0x05));
    assert_eq!(
        u32::from_le_bytes(coleco.bytes[0x0c..0x10].try_into().unwrap()),
        3_579_545
    );
    assert_eq!(
        u16::from_le_bytes(coleco.bytes[0x28..0x2a].try_into().unwrap()),
        0x0003
    );
    assert_eq!(coleco.bytes[0x2a], 15);
}

#[test]
fn malformed_or_unrepresentable_traces_reject_before_encoding() {
    let cancelled = AtomicBool::new(true);
    assert!(super::encode_sn76489(&trace(Vec::new(), 0), &cancelled).is_err());

    let mut invalidated = trace(Vec::new(), 0);
    invalidated.invalidated = Some(AudioTraceInvalidation::StateRestore);
    assert!(encode(&invalidated).is_err());

    let mut dropped = trace(Vec::new(), 0);
    dropped.dropped_events = 1;
    assert!(encode(&dropped).is_err());

    let past_end = trace(vec![sn(1, 0x90)], 0);
    assert!(encode(&past_end).is_err());

    let mut bad_stereo_port = trace(
        vec![AudioTraceEvent {
            cycle: 0,
            pc: 0,
            instruction_source: AudioTraceSource::Unknown,
            write: AudioTraceWrite::GameGearStereo {
                port: 0x07,
                value: 0,
            },
        }],
        0,
    );
    assert!(encode(&bad_stereo_port).is_err());

    bad_stereo_port.events.clear();
    bad_stereo_port.chip.tone_counter_clock_divider = 8;
    assert!(encode(&bad_stereo_port).is_err());

    let mut overlong = trace(Vec::new(), 1);
    overlong.cycle_hz = 1;
    overlong.end_cycle = MAX_SAMPLES + 1;
    assert!(encode(&overlong).is_err());
}

#[test]
fn long_waits_and_absolute_floor_quantization_have_no_accumulated_drift() {
    let mut long = trace(vec![sn(65_536, 0x90)], 131_072);
    long.cycle_hz = TICKS_PER_SECOND;
    let long_capture = encode(&long).unwrap();
    assert_eq!(
        &long_capture.bytes[HEADER_LEN + 26..HEADER_LEN + 29],
        &[0x61, 0xff, 0xff]
    );
    let long_log = crate::vgm::inspect(
        &long_capture.bytes,
        crate::ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(long_log.samples, 131_072);

    let mut fractional = trace(vec![sn(1, 0x90), sn(2, 0x91), sn(3, 0x92)], 10);
    fractional.cycle_hz = 1_000_003;
    let capture = encode(&fractional).unwrap();
    let expected = (u128::from(fractional.end_cycle) * u128::from(TICKS_PER_SECOND)
        / u128::from(fractional.cycle_hz)) as u64;
    let log = crate::vgm::inspect(
        &capture.bytes,
        crate::ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(log.samples, expected);
    assert_eq!(capture.metadata.total_samples, expected as u32);

    fractional.cycle_hz_denominator = 2;
    let rational = encode(&fractional).unwrap();
    let expected = u128::from(fractional.end_cycle) * u128::from(TICKS_PER_SECOND) * 2
        / u128::from(fractional.cycle_hz);
    assert_eq!(rational.metadata.total_samples, expected as u32);
    assert!(
        rational
            .metadata
            .quantization
            .contains("cycle_hz_denominator")
    );
    assert!(rational.metadata.limitations[1].contains("cycle_hz_denominator"));
}
