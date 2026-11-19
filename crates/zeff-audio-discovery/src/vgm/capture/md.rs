use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde::Serialize;
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceSource, AudioTraceStart, AudioTraceTiming, ChipAudioTrace,
    MdAudioTrace, MdTraceOrigin, MdTraceRegion, MdTraceReset, MdTraceSource, MdTraceWrite,
    MdTraceWriter,
};

use super::{
    MdCaptureMetadata, VgmCapture,
    stream::{self, EventCommand, Preamble, StreamConfig, put_u32},
};

const PREAMBLE_WRITES: usize = 217;
const LIMITATIONS: &[&str] = &[
    "The synthetic preamble initializes PSG and ordinary OPN2 registers; fresh-player reset is assumed. It does not restore native chip phases, counters, LFSR, timers, pending keys, busy state or output pipelines.",
    "VGM waits use floor(absolute_master_tick * 44100 / master_hz), including leading silence and the complete end interval. Equal-time register commands retain native order.",
    "OPN2 address-port writes are resolved through the shared native latch; their original bus events and all typed origins remain in native JSON. Only admitted data destinations are projected.",
    "This is a quantized register projection, without native PCM equivalence, exact external PSG/FM balance, DAC/filter equivalence, song identity or loop claims.",
    "PSG polarity, oscillator and tone-2 noise-clock phase behavior cannot be fully represented by VGM flags.",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MdVgmExport {
    pub capture: Option<VgmCapture>,
    pub unavailable: Vec<MdVgmUnavailable>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MdVgmUnavailable {
    pub code: &'static str,
    pub message: &'static str,
    pub first_event_index: Option<usize>,
}

#[derive(Clone, Copy)]
enum ProjectedWrite {
    Psg(u8),
    Opn2 { bank: u8, address: u8, value: u8 },
}

pub fn encode(trace: &MdAudioTrace, cancel: &AtomicBool) -> Result<MdVgmExport> {
    validate_md_trace(trace, cancel)?;
    let mut unavailable = Vec::new();
    let mut events = Vec::new();
    events.try_reserve_exact(trace.events.len())?;
    let mut latch = 0u16;
    let mut psg_write_count = 0;
    let mut opn2_address_write_count = 0;
    let mut opn2_register_write_count = 0;
    for (index, event) in trace.events.iter().enumerate() {
        ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
        let write = match event.write {
            MdTraceWrite::Psg { value, .. } => {
                psg_write_count += 1;
                Some(ProjectedWrite::Psg(value))
            }
            MdTraceWrite::Opn2Port { port, value, .. } if port & 1 == 0 => {
                latch = (u16::from(port & 2) << 7) | u16::from(value);
                opn2_address_write_count += 1;
                None
            }
            MdTraceWrite::Opn2Port { value, .. } => {
                let bank = (latch >> 8) as u8;
                let address = latch as u8;
                if accepted_register(bank, address) {
                    opn2_register_write_count += 1;
                    Some(ProjectedWrite::Opn2 {
                        bank,
                        address,
                        value,
                    })
                } else {
                    reject(
                        &mut unavailable,
                        "opn2_unmapped_register",
                        "An OPN2 data write selects a destination outside the admitted register projection.",
                        Some(index),
                    );
                    None
                }
            }
            MdTraceWrite::Opn2Reset { .. } => {
                reject(
                    &mut unavailable,
                    "opn2_reset",
                    "An in-stream OPN2 reset cannot be represented by the admitted VGM commands.",
                    Some(index),
                );
                latch = 0;
                None
            }
        };
        if let Some(write) = write {
            events.push(AudioTraceEvent {
                cycle: event.cycle,
                pc: event.pc,
                instruction_source: event.instruction_source,
                write,
            });
        }
    }
    let total_samples = samples_at(trace.end_cycle, trace.cycle_hz);
    if total_samples > u128::from(crate::vgm::MAX_SAMPLES) {
        reject(
            &mut unavailable,
            "duration_limit",
            "The complete native trace exceeds the VGM duration limit.",
            None,
        );
    }
    if !unavailable.is_empty() {
        return Ok(MdVgmExport {
            capture: None,
            unavailable,
        });
    }
    let master = trace.chip.master_hz;
    let psg_clock = ((master + 7) / 15) as u32;
    let opn2_clock = ((master + 3) / 7) as u32;
    let mut header = stream::header();
    put_u32(&mut header, 0x0c, psg_clock);
    put_u32(&mut header, 0x2c, opn2_clock);
    header[0x28..0x2a].copy_from_slice(&9u16.to_le_bytes());
    header[0x2a] = 16;
    header[0x2b] = 4;
    let writes = ChipAudioTrace {
        generation: trace.generation,
        cycle_hz: trace.cycle_hz,
        cycle_hz_denominator: 1,
        chip: trace.chip,
        timing: trace.timing,
        start: trace.start,
        end_cycle: trace.end_cycle,
        events,
        dropped_events: 0,
        invalidated: None,
    };
    let preamble = reset_preamble();
    let capture = stream::encode(
        &writes,
        StreamConfig {
            header,
            preamble: preamble.as_slice(),
            preamble_write_count: preamble.write_count,
            sn76489_flags: Some(4),
            huc6280: None,
            wonder_swan: None,
            game_boy: None,
            nes: None,
            md: Some(MdCaptureMetadata {
                region: match trace.chip.region {
                    MdTraceRegion::NtscUs => "ntsc_us",
                    MdTraceRegion::NtscJapan => "ntsc_japan",
                    MdTraceRegion::Pal => "pal",
                },
                master_clock_hz: master,
                psg_clock_hz_numerator: master,
                psg_clock_hz_denominator: 15,
                psg_vgm_clock_hz: psg_clock,
                opn2_clock_hz_numerator: master,
                opn2_clock_hz_denominator: 7,
                opn2_vgm_clock_hz: opn2_clock,
                clock_rounding: "nearest_integer_hz",
                reset: "zeff_power_on_v1",
                native_event_count: trace.events.len() as u32,
                psg_write_count,
                opn2_address_write_count,
                opn2_register_write_count,
            }),
            limitations: LIMITATIONS,
        },
        cancel,
        |write| match *write {
            ProjectedWrite::Psg(value) => EventCommand::two([0x50, value]),
            ProjectedWrite::Opn2 {
                bank,
                address,
                value,
            } => EventCommand::three([0x52 + bank, address, value]),
        },
    )?;
    Ok(MdVgmExport {
        capture: Some(capture),
        unavailable,
    })
}

pub fn validate_md_trace(trace: &MdAudioTrace, cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "MD trace validation cancelled"
    );
    trace.validate_complete()?;
    let master = match trace.chip.region {
        MdTraceRegion::NtscUs | MdTraceRegion::NtscJapan => 53_693_175,
        MdTraceRegion::Pal => 53_203_424,
    };
    ensure!(
        trace.start == AudioTraceStart::Reset
            && trace.timing == AudioTraceTiming::BusServiceBoundary
            && trace.cycle_hz_denominator == 1
            && u64::from(trace.cycle_hz) == master
            && trace.chip.master_hz == master
            && trace.chip.psg_divisor == 240
            && trace.chip.opn2_divisor == 7
            && trace.chip.reset == MdTraceReset::ZeffPowerOnV1,
        "MD trace requires the canonical reset bus-service-boundary descriptor"
    );
    for event in &trace.events {
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "MD trace validation cancelled"
        );
        let origin = match event.write {
            MdTraceWrite::Psg { origin, .. } | MdTraceWrite::Opn2Reset { origin } => origin,
            MdTraceWrite::Opn2Port { origin, port, .. } => {
                ensure!(port <= 3, "MD trace has an invalid OPN2 port");
                origin
            }
        };
        ensure!(
            event.pc == origin.boundary_pc && event.instruction_source == AudioTraceSource::Unknown,
            "MD trace generic provenance does not match its typed origin"
        );
        validate_origin(origin)?;
    }
    Ok(())
}

fn validate_origin(origin: MdTraceOrigin) -> Result<()> {
    let pc = origin.boundary_pc;
    match origin.source {
        MdTraceSource::EffectiveRom { offset } => {
            ensure!(
                offset < 0x40_0000,
                "MD effective ROM origin exceeds native ROM bounds"
            );
        }
        MdTraceSource::MainRam { offset } => {
            ensure!(
                offset < 0x1_0000,
                "MD main RAM origin exceeds native RAM bounds"
            );
        }
        MdTraceSource::SoundRam { offset } => {
            ensure!(
                origin.writer == MdTraceWriter::Z80 && offset < 0x2000,
                "MD sound RAM origin has an invalid writer or offset"
            );
        }
        MdTraceSource::Unknown => (),
    }
    match origin.writer {
        MdTraceWriter::M68000 => {
            let address = pc & 0xff_ffff;
            ensure!(
                match origin.source {
                    MdTraceSource::EffectiveRom { offset } => address == offset,
                    MdTraceSource::MainRam { offset } =>
                        address >= 0xe0_0000 && address & 0xffff == offset,
                    MdTraceSource::SoundRam { .. } => false,
                    MdTraceSource::Unknown => true,
                },
                "MD M68000 origin is inconsistent with its masked boundary address"
            );
        }
        MdTraceWriter::Z80 => {
            ensure!(
                pc <= u32::from(u16::MAX),
                "MD Z80 boundary PC exceeds 16 bits"
            );
            ensure!(
                match origin.source {
                    MdTraceSource::SoundRam { offset } =>
                        pc <= 0x3fff && pc & 0x1fff == u32::from(offset),
                    MdTraceSource::EffectiveRom { offset } | MdTraceSource::MainRam { offset } => {
                        pc >= 0x8000 && pc & 0x7fff == offset & 0x7fff
                    }
                    MdTraceSource::Unknown => true,
                },
                "MD Z80 origin is inconsistent with its boundary address"
            );
        }
    }
    Ok(())
}

fn reject(
    rows: &mut Vec<MdVgmUnavailable>,
    code: &'static str,
    message: &'static str,
    first_event_index: Option<usize>,
) {
    if !rows.iter().any(|row| row.code == code) {
        rows.push(MdVgmUnavailable {
            code,
            message,
            first_event_index,
        });
    }
}

fn samples_at(tick: u64, master_hz: u32) -> u128 {
    u128::from(tick) * 44_100 / u128::from(master_hz)
}

fn accepted_register(bank: u8, address: u8) -> bool {
    (bank == 0 && matches!(address, 0x22 | 0x24..=0x28 | 0x2a | 0x2b | 0xa8..=0xaa | 0xac..=0xae))
        || (matches!(address, 0x30..=0x9f) && address & 3 != 3)
        || matches!(address, 0xa0..=0xa2 | 0xa4..=0xa6 | 0xb0..=0xb2 | 0xb4..=0xb6)
}

fn reset_preamble() -> Preamble<639> {
    let mut preamble = Preamble {
        bytes: [0; 639],
        len: 0,
        write_count: 0,
    };
    for value in [
        0x80, 0, 0xa0, 0, 0xc0, 0, 0x9f, 0xbf, 0xdf, 0xff, 0xe0, 0x80,
    ] {
        preamble.push([0x50, value]);
    }
    for (address, value) in [
        (0x22, 0),
        (0x24, 0),
        (0x25, 0),
        (0x26, 0),
        (0x27, 0),
        (0x2a, 0x80),
        (0x2b, 0),
    ] {
        preamble.push([0x52, address, value]);
    }
    for bank in 0..=1 {
        for address in 0x30..=0x9f {
            if address & 3 != 3 {
                preamble.push([0x52 + bank, address, 0]);
            }
        }
        for channel in 0..3 {
            for (address, value) in [
                (0xa4 + channel, 0),
                (0xa0 + channel, 0),
                (0xb0 + channel, 0),
                (0xb4 + channel, 0xc0),
            ] {
                preamble.push([0x52 + bank, address, value]);
            }
        }
    }
    for channel in 0..3 {
        preamble.push([0x52, 0xac + channel, 0]);
        preamble.push([0x52, 0xa8 + channel, 0]);
    }
    debug_assert_eq!(preamble.write_count, PREAMBLE_WRITES);
    preamble
}
