use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use zeff_emu_common::audio_trace::{
    AudioTraceStart, Huc6280AudioTrace, Huc6280ResetState, Huc6280TraceChip, Huc6280TraceRevision,
};

use super::{
    Huc6280CaptureMetadata, VgmCapture,
    stream::{self, EventCommand, Preamble, StreamConfig, put_u32},
};

const LIMITATIONS: &[&str] = &[
    "The trace begins at emulator reset; the preamble reconstructs reset register and wave-RAM values but is not a guest write record.",
    "VGM waits use floor(absolute_cycle * 44100 * cycle_hz_denominator / cycle_hz); source PCs, physical addresses and source locations remain in the trace.",
    "The VGM HuC6280 clock is the nearest integer to the exact oscillator clock; the source clock ratio is retained in metadata.",
    "VGM cannot specify the HuC6280/HuC6280A revision or reconstruct oscillator, noise, LFO, gain-scan and resampler phase; the preamble itself can alter hidden chip state.",
    "The trace preserves raw writes, including ignored channel selections and register aliases; playback behavior depends on the external player's HuC6280 implementation.",
    "This capture contains register writes only; it does not establish song identity, loop metadata, or PCM equivalence.",
];

pub fn encode(trace: &Huc6280AudioTrace, cancel: &AtomicBool) -> Result<VgmCapture> {
    ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
    validate_trace(trace, cancel)?;
    let chip = &trace.chip;
    let rounded_clock = rounded_clock(chip)?;
    let mut header = stream::header();
    put_u32(&mut header, 0xa4, rounded_clock);
    let preamble = reset_preamble();
    stream::encode(
        trace,
        StreamConfig {
            header,
            preamble: preamble.as_slice(),
            preamble_write_count: preamble.write_count,
            sn76489_flags: None,
            huc6280: Some(Huc6280CaptureMetadata {
                clock_hz_numerator: chip.clock_hz_numerator,
                clock_hz_denominator: chip.clock_hz_denominator,
                vgm_clock_hz: rounded_clock,
                clock_rounding: "nearest integer; exact halves round upward",
                revision: match chip.revision {
                    Huc6280TraceRevision::HuC6280 => "huc6280",
                    Huc6280TraceRevision::HuC6280A => "huc6280a",
                },
            }),
            wonder_swan: None,
            game_boy: None,
            limitations: LIMITATIONS,
        },
        cancel,
        |write| EventCommand::three([0xb9, write.register, write.value]),
    )
}

fn validate_trace(trace: &Huc6280AudioTrace, cancel: &AtomicBool) -> Result<()> {
    trace.validate_complete()?;
    ensure!(
        matches!(trace.start, AudioTraceStart::Reset),
        "VGM capture requires a reset-to-end audio trace"
    );
    let chip = &trace.chip;
    ensure!(
        chip.master_clock_divisor == 6 && chip.internal_master_clock_divisor == 3,
        "HuC6280 clock-divider configuration is unsupported by this VGM writer"
    );
    let master_scaled =
        u128::from(trace.cycle_hz).checked_mul(u128::from(chip.clock_hz_denominator));
    let chip_scaled = u128::from(chip.clock_hz_numerator)
        .checked_mul(u128::from(trace.cycle_hz_denominator))
        .and_then(|value| value.checked_mul(u128::from(chip.master_clock_divisor)));
    ensure!(
        master_scaled.is_some() && master_scaled == chip_scaled,
        "HuC6280 oscillator clock does not match the trace master clock"
    );
    ensure!(
        chip.reset == Huc6280ResetState::default(),
        "HuC6280 VGM capture requires the canonical emulator reset state"
    );
    for event in &trace.events {
        ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
        let write = event.write;
        ensure!(
            (0x1fe800..=0x1febff).contains(&write.physical_address),
            "HuC6280 trace write is outside the PSG address window"
        );
        ensure!(
            write.register == (write.physical_address & 0x0f) as u8,
            "HuC6280 trace register does not match its physical address"
        );
    }
    Ok(())
}

fn rounded_clock(chip: &Huc6280TraceChip) -> Result<u32> {
    let rounded = (u128::from(chip.clock_hz_numerator) + u128::from(chip.clock_hz_denominator) / 2)
        / u128::from(chip.clock_hz_denominator);
    ensure!(
        (1..=0x3fff_ffff).contains(&rounded),
        "HuC6280 rounded clock is zero or collides with VGM chip flags"
    );
    Ok(rounded as u32)
}

fn reset_preamble() -> Preamble<723> {
    let mut preamble = Preamble {
        bytes: [0; 723],
        len: 0,
        write_count: 0,
    };
    preamble.push([0xb9, 1, 0]);
    preamble.push([0xb9, 9, 0x80]);
    preamble.push([0xb9, 8, 0]);
    for channel in 0..6 {
        preamble.push([0xb9, 0, channel]);
        // Toggle DDA to restore its hold value and reset the wave write cursor.
        preamble.push([0xb9, 4, 0x40]);
        preamble.push([0xb9, 6, 0]);
        preamble.push([0xb9, 4, 0]);
        preamble.push([0xb9, 2, 0]);
        preamble.push([0xb9, 3, 0]);
        preamble.push([0xb9, 5, 0]);
        if channel >= 4 {
            preamble.push([0xb9, 7, 0]);
        }
        for _ in 0..32 {
            preamble.push([0xb9, 6, 0]);
        }
    }
    preamble.push([0xb9, 9, 0]);
    preamble.push([0xb9, 0, 0]);
    preamble
}
