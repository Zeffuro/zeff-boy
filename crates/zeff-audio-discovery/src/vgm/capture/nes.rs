use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde::Serialize;
use zeff_emu_common::audio_trace::{
    AudioTraceSource, AudioTraceStart, AudioTraceTiming, NesAudioTrace, NesTraceOrigin,
    NesTraceRegion, NesTraceReset, NesTraceWrite,
};

use super::{
    NesCaptureMetadata, VgmCapture,
    stream::{self, EventCommand, StreamConfig, put_u32},
};

const NTSC_CLOCK: (u64, u32, u32) = (19_687_500, 11, 1_789_773);
const PAL_CLOCK: (u64, u32, u32) = (53_203_425, 32, 1_662_607);
const DENDY_CLOCK: (u64, u32) = (3_546_895, 2);

const LIMITATIONS: &[&str] = &[
    "The VGM preamble writes only an idempotent $4015=0 DMC disable; it is not a guest event.",
    "VGM waits use floor(absolute_cycle * 44100 * cycle_hz_denominator / cycle_hz); source PCs, origins and status-read observations remain in trace JSON.",
    "VGM cannot recreate the captured APU frame-counter phase, CPU parity or status-read side effects. Guest $4017 writes remain ordered VGM register commands, but this is a register projection rather than exact native PCM.",
    "DMC enable and DMC fetches are rejected. Direct DAC $4011 writes are retained as NES APU register writes.",
    "This capture contains executed base-APU register writes only; it does not establish song identity, loop metadata, mapper behavior, expansion audio, or external-player PCM equivalence.",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NesVgmExport {
    pub capture: Option<VgmCapture>,
    pub unavailable: Vec<NesVgmUnavailable>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NesVgmUnavailable {
    pub code: &'static str,
    pub message: &'static str,
    pub first_event_index: Option<usize>,
}

pub fn encode(trace: &NesAudioTrace, cancel: &AtomicBool) -> Result<NesVgmExport> {
    validate(trace, cancel)?;
    let mut unavailable = Vec::new();
    let mut reject = |code, message, first_event_index| {
        if !unavailable
            .iter()
            .any(|row: &NesVgmUnavailable| row.code == code)
        {
            unavailable.push(NesVgmUnavailable {
                code,
                message,
                first_event_index,
            });
        }
    };
    if trace.chip.region == NesTraceRegion::Dendy {
        reject(
            "dendy_region",
            "Dendy NES APU timing has no qualified VGM clock contract.",
            None,
        );
    }
    for (index, event) in trace.events.iter().enumerate() {
        ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
        match event.write {
            NesTraceWrite::Register {
                address: 0x4015,
                value,
                ..
            } if value & 0x10 != 0 => reject(
                "dmc_enabled",
                "VGM NES APU projection cannot reproduce an enabled DMC channel.",
                Some(index),
            ),
            NesTraceWrite::DmcFetch { .. } => reject(
                "dmc_fetch",
                "VGM NES APU projection cannot reproduce fetched DMC sample bytes.",
                Some(index),
            ),
            _ => (),
        }
    }
    if trace.chip.region != NesTraceRegion::Dendy
        && u128::from(trace.end_cycle)
            * u128::from(44_100_u32)
            * u128::from(trace.cycle_hz_denominator)
            / u128::from(trace.cycle_hz)
            > u128::from(crate::vgm::MAX_SAMPLES)
    {
        reject(
            "duration_limit",
            "The complete native trace exceeds the VGM duration limit.",
            None,
        );
    }
    if !unavailable.is_empty() {
        return Ok(NesVgmExport {
            capture: None,
            unavailable,
        });
    }

    let (_, _, vgm_clock_hz) = clock(trace)?;
    let mut header = stream::header();
    put_u32(&mut header, 0x84, vgm_clock_hz);
    let mut writes = trace.clone();
    writes
        .events
        .retain(|event| matches!(event.write, NesTraceWrite::Register { .. }));
    let status_read_observation_count = trace
        .events
        .iter()
        .filter(|event| matches!(event.write, NesTraceWrite::StatusRead { .. }))
        .count() as u32;
    let capture = stream::encode(
        &writes,
        StreamConfig {
            header,
            preamble: &[0xb4, 0x15, 0],
            preamble_write_count: 1,
            sn76489_flags: None,
            huc6280: None,
            wonder_swan: None,
            game_boy: None,
            nes: Some(NesCaptureMetadata {
                region: match trace.chip.region {
                    NesTraceRegion::Ntsc => "ntsc",
                    NesTraceRegion::Pal => "pal",
                    NesTraceRegion::Dendy => unreachable!("unavailable Dendy trace"),
                },
                clock_hz_numerator: trace.chip.clock_hz_numerator,
                clock_hz_denominator: trace.chip.clock_hz_denominator,
                vgm_clock_hz,
                clock_rounding: "nearest_integer_hz",
                reset: "zeff_power_on_v1",
                status_read_observation_count,
            }),
            limitations: LIMITATIONS,
        },
        cancel,
        |write| match *write {
            NesTraceWrite::Register { address, value, .. } => {
                EventCommand::three([0xb4, (address - 0x4000) as u8, value])
            }
            _ => unreachable!("validated NES VGM projection contains only registers"),
        },
    )?;
    Ok(NesVgmExport {
        capture: Some(capture),
        unavailable,
    })
}

fn clock(trace: &NesAudioTrace) -> Result<(u64, u32, u32)> {
    native_clock(trace)?;
    let clock = match trace.chip.region {
        NesTraceRegion::Ntsc => NTSC_CLOCK,
        NesTraceRegion::Pal => PAL_CLOCK,
        NesTraceRegion::Dendy => anyhow::bail!("Dendy NES APU timing has no qualified VGM clock"),
    };
    Ok(clock)
}

fn native_clock(trace: &NesAudioTrace) -> Result<(u64, u32)> {
    let clock = match trace.chip.region {
        NesTraceRegion::Ntsc => (NTSC_CLOCK.0, NTSC_CLOCK.1),
        NesTraceRegion::Pal => (PAL_CLOCK.0, PAL_CLOCK.1),
        NesTraceRegion::Dendy => DENDY_CLOCK,
    };
    ensure!(
        (
            trace.chip.clock_hz_numerator,
            trace.chip.clock_hz_denominator
        ) == (clock.0, clock.1),
        "NES trace clock does not match its qualified VGM region"
    );
    Ok(clock)
}

fn validate(trace: &NesAudioTrace, cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
    trace.validate_complete()?;
    ensure!(
        trace.start == AudioTraceStart::Reset
            && trace.timing == AudioTraceTiming::CpuBusCycleBoundary
            && u64::from(trace.cycle_hz) == trace.chip.clock_hz_numerator
            && trace.cycle_hz_denominator == trace.chip.clock_hz_denominator,
        "NES VGM capture requires a complete reset CPU-bus-cycle trace"
    );
    ensure!(
        trace.chip.reset == NesTraceReset::ZeffPowerOnV1
            && trace.chip.initial_cpu_cycle == 7
            && trace.chip.initial_cpu_cycle_odd
            && trace.chip.initial_apu_frame_cycle == 9
            && !trace.chip.initial_half_rate_timer_clock,
        "NES VGM capture requires the canonical fresh APU descriptor"
    );
    native_clock(trace)?;
    for event in &trace.events {
        ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
        ensure!(
            event.cycle < trace.end_cycle && event.pc <= u32::from(u16::MAX),
            "NES trace event is outside its native interval or address space"
        );
        match event.write {
            NesTraceWrite::Register {
                address, odd_cycle, ..
            } => ensure!(
                matches!(address, 0x4000..=0x4013 | 0x4015 | 0x4017)
                    && odd_cycle == event.cycle.is_multiple_of(2),
                "unsupported NES APU register or bus-cycle parity"
            ),
            NesTraceWrite::StatusRead { value, origin } => {
                ensure!(value & 0x20 == 0, "invalid native NES APU status value");
                ensure!(
                    origin == NesTraceOrigin::Cpu
                        || (event.pc == 0 && event.instruction_source == AudioTraceSource::Unknown),
                    "autonomous NES status read has an attributed CPU instruction"
                );
            }
            NesTraceWrite::DmcFetch { address, .. } => ensure!(
                address >= 0x8000
                    && event.pc == 0
                    && event.instruction_source == AudioTraceSource::Unknown,
                "unsupported NES DMC fetch address or provenance"
            ),
        }
    }
    Ok(())
}
