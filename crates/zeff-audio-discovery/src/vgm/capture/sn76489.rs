use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use zeff_emu_common::audio_trace::{
    AudioTrace, AudioTraceStart, AudioTraceWrite, MAX_AUDIO_TRACE_EVENTS, Sn76489TraceChip,
    Sn76489ZeroPeriod,
};

use super::{
    VgmCapture,
    stream::{self, EventCommand, Preamble, StreamConfig, put_u32},
};

const LIMITATIONS: &[&str] = &[
    "The trace begins at emulator reset; the preamble reconstructs reset register values but is not a guest write record.",
    "VGM waits use floor(absolute_cycle * 44100 / cycle_hz); source PCs and source locations are retained by the trace, not the VGM stream.",
    "VGM does not encode oscillator phase, LFSR state, reset countdowns, period-one behavior, or the core-specific tone-2 noise-clock edge behavior.",
    "Global output negation is disabled to preserve the cores' positive tone convention; independent noise polarity cannot be represented by that flag.",
    "This capture contains register writes only; it does not establish song identity, loop metadata, or PCM equivalence.",
];

const RATIONAL_LIMITATIONS: &[&str] = &[
    LIMITATIONS[0],
    "VGM waits use floor(absolute_cycle * 44100 * cycle_hz_denominator / cycle_hz); source PCs and source locations are retained by the trace, not the VGM stream.",
    LIMITATIONS[2],
    LIMITATIONS[3],
    LIMITATIONS[4],
];

pub fn encode(trace: &AudioTrace, cancel: &AtomicBool) -> Result<VgmCapture> {
    ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
    validate_trace(trace, cancel)?;
    let flags = sn76489_flags(&trace.chip);
    let preamble = reset_preamble(&trace.chip);
    let mut header = stream::header();
    put_u32(&mut header, 0x0c, trace.chip.clock_hz);
    header[0x28..0x2a].copy_from_slice(&trace.chip.feedback_mask.to_le_bytes());
    header[0x2a] = trace.chip.shift_register_width;
    header[0x2b] = flags;
    stream::encode(
        trace,
        StreamConfig {
            header,
            preamble: preamble.as_slice(),
            preamble_write_count: preamble.write_count,
            sn76489_flags: Some(flags),
            huc6280: None,
            wonder_swan: None,
            game_boy: None,
            nes: None,
            limitations: if trace.cycle_hz_denominator == 1 {
                LIMITATIONS
            } else {
                RATIONAL_LIMITATIONS
            },
        },
        cancel,
        |write| match *write {
            AudioTraceWrite::Sn76489 { value, .. } => EventCommand::two([0x50, value]),
            AudioTraceWrite::GameGearStereo { value, .. } => EventCommand::two([0x4f, value]),
        },
    )
}

fn validate_trace(trace: &AudioTrace, cancel: &AtomicBool) -> Result<()> {
    trace.validate_complete()?;
    ensure!(
        matches!(trace.start, AudioTraceStart::Reset),
        "VGM capture requires a reset-to-end audio trace"
    );
    ensure!(
        trace.events.len() <= MAX_AUDIO_TRACE_EVENTS,
        "audio trace exceeds the supported event limit"
    );
    for event in &trace.events {
        ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
        if let AudioTraceWrite::GameGearStereo { port, .. } = event.write {
            ensure!(
                trace.chip.stereo,
                "audio trace has Game Gear stereo without stereo support"
            );
            ensure!(
                port == 0x06,
                "audio trace has an invalid Game Gear stereo port"
            );
        }
    }
    validate_chip(&trace.chip)
}

fn validate_chip(chip: &Sn76489TraceChip) -> Result<()> {
    ensure!(
        chip.clock_hz <= 0x3fff_ffff,
        "SN76489 clock collides with VGM chip flags"
    );
    ensure!(
        chip.feedback_mask != 0,
        "SN76489 feedback mask must be nonzero"
    );
    ensure!(
        (1..=16).contains(&chip.shift_register_width),
        "SN76489 shift-register width is unsupported"
    );
    let lfsr_limit = 1u32 << chip.shift_register_width;
    ensure!(
        u32::from(chip.feedback_mask) < lfsr_limit,
        "SN76489 feedback mask exceeds its shift-register width"
    );
    ensure!(
        chip.tone_counter_clock_divider == 16,
        "SN76489 clock-divider configuration is unsupported by this VGM writer"
    );
    for &period in &chip.reset.tone_periods {
        ensure!(
            period <= 0x03ff,
            "SN76489 reset tone period exceeds 10 bits"
        );
    }
    for &volume in &chip.reset.volumes {
        ensure!(volume <= 0x0f, "SN76489 reset volume exceeds 4 bits");
    }
    ensure!(
        chip.reset.noise_control <= 0x07,
        "SN76489 reset noise control exceeds 3 bits"
    );
    ensure!(
        chip.reset.latched_register <= 7,
        "SN76489 reset latch is invalid"
    );
    ensure!(
        chip.reset.noise_lfsr != 0 && u32::from(chip.reset.noise_lfsr) < lfsr_limit,
        "SN76489 reset LFSR does not fit its shift-register width"
    );
    Ok(())
}

fn sn76489_flags(chip: &Sn76489TraceChip) -> u8 {
    let mut flags = match chip.zero_period {
        Sn76489ZeroPeriod::ConstantHigh => 0,
        Sn76489ZeroPeriod::Period1024 => 1,
    };
    if !chip.stereo {
        flags |= 1 << 2;
    }
    flags
}

fn reset_preamble(chip: &Sn76489TraceChip) -> Preamble<26> {
    let mut preamble = Preamble {
        bytes: [0; 26],
        len: 0,
        write_count: 0,
    };
    if chip.stereo {
        preamble.push([0x4f, chip.reset.stereo_control]);
    }
    for (channel, period) in chip.reset.tone_periods.iter().copied().enumerate() {
        preamble.push([0x50, 0x80 | ((channel as u8) << 5) | (period as u8 & 0x0f)]);
        preamble.push([0x50, (period >> 4) as u8]);
    }
    for (channel, volume) in chip.reset.volumes.iter().copied().enumerate() {
        preamble.push([0x50, 0x90 | ((channel as u8) << 5) | volume]);
    }
    preamble.push([0x50, 0xe0 | chip.reset.noise_control]);
    preamble.push([0x50, latch_byte(&chip.reset)]);
    preamble
}

fn latch_byte(reset: &zeff_emu_common::audio_trace::Sn76489ResetState) -> u8 {
    let register = reset.latched_register;
    let low = match register {
        0 | 2 | 4 => reset.tone_periods[usize::from(register >> 1)] as u8 & 0x0f,
        1 | 3 | 5 | 7 => reset.volumes[usize::from(register >> 1)],
        6 => reset.noise_control,
        _ => unreachable!("validated SN76489 latch"),
    };
    0x80 | (register << 4) | low
}
