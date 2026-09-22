use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use zeff_emu_common::audio_trace::{AudioTraceTiming, ChipAudioTrace};

use super::{
    GameBoyCaptureMetadata, HEADER_LEN, Huc6280CaptureMetadata, NesCaptureMetadata, VGM_VERSION,
    VgmCapture, VgmCaptureMetadata, WonderSwanCaptureMetadata,
};
use crate::vgm::{MAX_COMMANDS, MAX_ROM_BYTES, MAX_SAMPLES, TICKS_PER_SECOND};

const MAX_WAIT: u64 = u16::MAX as u64;
const DATA_OFFSET_FIELD: usize = 0x34;
const TOTAL_SAMPLES_OFFSET: usize = 0x18;

pub(super) struct StreamConfig<'a> {
    pub header: [u8; HEADER_LEN],
    pub preamble: &'a [u8],
    pub preamble_write_count: usize,
    pub sn76489_flags: Option<u8>,
    pub huc6280: Option<Huc6280CaptureMetadata>,
    pub wonder_swan: Option<WonderSwanCaptureMetadata>,
    pub game_boy: Option<GameBoyCaptureMetadata>,
    pub nes: Option<NesCaptureMetadata>,
    pub limitations: &'static [&'static str],
}

pub(super) struct EventCommand {
    bytes: [u8; 4],
    len: usize,
}

impl EventCommand {
    pub const fn two(bytes: [u8; 2]) -> Self {
        Self {
            bytes: [bytes[0], bytes[1], 0, 0],
            len: 2,
        }
    }

    pub const fn three(bytes: [u8; 3]) -> Self {
        Self {
            bytes: [bytes[0], bytes[1], bytes[2], 0],
            len: 3,
        }
    }

    pub const fn four(bytes: [u8; 4]) -> Self {
        Self { bytes, len: 4 }
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

pub(super) fn header() -> [u8; HEADER_LEN] {
    let mut header = [0; HEADER_LEN];
    header[..4].copy_from_slice(b"Vgm ");
    put_u32(&mut header, 8, VGM_VERSION);
    put_u32(
        &mut header,
        DATA_OFFSET_FIELD,
        (HEADER_LEN - DATA_OFFSET_FIELD) as u32,
    );
    header
}

pub(super) fn encode<C, W>(
    trace: &ChipAudioTrace<C, W>,
    config: StreamConfig<'_>,
    cancel: &AtomicBool,
    command: impl Fn(&W) -> EventCommand,
) -> Result<VgmCapture> {
    let total_samples = samples_at(trace.end_cycle, trace.cycle_hz, trace.cycle_hz_denominator)?;
    ensure!(
        total_samples <= MAX_SAMPLES,
        "audio trace duration exceeds the VGM capture limit"
    );
    ensure!(
        total_samples <= u64::from(u32::MAX),
        "audio trace duration does not fit the VGM sample field"
    );

    let wait_upper_bound = trace
        .events
        .len()
        .checked_add(ceil_div(total_samples, MAX_WAIT)? as usize)
        .ok_or_else(|| anyhow::anyhow!("VGM command count overflows"))?;
    let command_upper_bound = config
        .preamble_write_count
        .checked_add(trace.events.len())
        .and_then(|count| count.checked_add(wait_upper_bound))
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| anyhow::anyhow!("VGM command count overflows"))?;
    ensure!(
        command_upper_bound <= MAX_COMMANDS as usize,
        "audio trace exceeds the VGM command limit"
    );
    let capacity = HEADER_LEN
        .checked_add(config.preamble.len())
        .and_then(|size| size.checked_add(trace.events.len().checked_mul(4)?))
        .and_then(|size| size.checked_add(wait_upper_bound.checked_mul(3)?))
        .and_then(|size| size.checked_add(1))
        .ok_or_else(|| anyhow::anyhow!("VGM output size overflows"))?;

    let mut bytes = Vec::new();
    ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
    bytes.try_reserve_exact(capacity)?;
    bytes.extend_from_slice(&config.header);
    bytes.extend_from_slice(config.preamble);

    let mut previous_samples = 0;
    let mut wait_command_count = 0u32;
    let mut guest_write_count = 0u32;
    for event in &trace.events {
        ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
        let target_samples = samples_at(event.cycle, trace.cycle_hz, trace.cycle_hz_denominator)?;
        emit_wait(
            &mut bytes,
            target_samples - previous_samples,
            &mut wait_command_count,
            cancel,
        )?;
        bytes.extend_from_slice(command(&event.write).as_slice());
        guest_write_count = guest_write_count
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("VGM guest-write count overflows"))?;
        previous_samples = target_samples;
    }
    emit_wait(
        &mut bytes,
        total_samples - previous_samples,
        &mut wait_command_count,
        cancel,
    )?;
    ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
    bytes.push(0x66);

    let command_count = (config.preamble_write_count as u64)
        .checked_add(u64::from(guest_write_count))
        .and_then(|count| count.checked_add(u64::from(wait_command_count)))
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| anyhow::anyhow!("VGM command count overflows"))?;
    ensure!(
        command_count <= u64::from(MAX_COMMANDS),
        "audio trace exceeds the VGM command limit"
    );
    ensure!(
        bytes.len() <= MAX_ROM_BYTES,
        "VGM output exceeds the source-size limit"
    );
    let eof_offset = bytes.len() as u32 - 4;
    put_u32(&mut bytes, 4, eof_offset);
    put_u32(&mut bytes, TOTAL_SAMPLES_OFFSET, total_samples as u32);

    Ok(VgmCapture {
        bytes,
        metadata: VgmCaptureMetadata {
            schema: "zeff-vgm-capture/1",
            version: VGM_VERSION,
            trace_generation: trace.generation,
            timing: timing_name(trace.timing),
            cycle_hz: trace.cycle_hz,
            cycle_hz_denominator: trace.cycle_hz_denominator,
            end_cycle: trace.end_cycle,
            total_samples: total_samples as u32,
            quantization: if trace.cycle_hz_denominator == 1 {
                "floor(absolute_cycle * 44100 / cycle_hz)"
            } else {
                "floor(absolute_cycle * 44100 * cycle_hz_denominator / cycle_hz)"
            },
            sn76489_flags: config.sn76489_flags,
            huc6280: config.huc6280,
            wonder_swan: config.wonder_swan,
            game_boy: config.game_boy,
            nes: config.nes,
            preamble_write_count: config.preamble_write_count as u32,
            guest_write_count,
            wait_command_count,
            command_count: command_count as u32,
            limitations: config.limitations,
        },
    })
}

fn samples_at(cycle: u64, cycle_hz: u32, denominator: u32) -> Result<u64> {
    ensure!(
        cycle_hz != 0 && denominator != 0,
        "audio trace has a zero clock"
    );
    let samples = u128::from(cycle)
        .checked_mul(u128::from(TICKS_PER_SECOND))
        .and_then(|value| value.checked_mul(u128::from(denominator)))
        .ok_or_else(|| anyhow::anyhow!("VGM sample conversion overflows"))?
        / u128::from(cycle_hz);
    u64::try_from(samples).map_err(|_| anyhow::anyhow!("VGM sample conversion overflows"))
}

fn ceil_div(value: u64, divisor: u64) -> Result<u64> {
    value
        .checked_add(divisor - 1)
        .map(|value| value / divisor)
        .ok_or_else(|| anyhow::anyhow!("VGM wait count overflows"))
}

fn emit_wait(
    bytes: &mut Vec<u8>,
    mut samples: u64,
    command_count: &mut u32,
    cancel: &AtomicBool,
) -> Result<()> {
    while samples != 0 {
        ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
        let wait = samples.min(MAX_WAIT) as u16;
        bytes.push(0x61);
        bytes.extend_from_slice(&wait.to_le_bytes());
        *command_count += 1;
        samples -= u64::from(wait);
    }
    Ok(())
}

fn timing_name(timing: AudioTraceTiming) -> &'static str {
    match timing {
        AudioTraceTiming::InstructionBoundary => "instruction_boundary",
        AudioTraceTiming::IoWriteCompletion => "io_write_completion",
        AudioTraceTiming::MemoryWriteCompletion => "memory_write_completion",
        AudioTraceTiming::BusServiceBoundary => "bus_service_boundary",
        AudioTraceTiming::CpuBusCycleBoundary => "cpu_bus_cycle_boundary",
    }
}

pub(super) fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub(super) struct Preamble<const CAPACITY: usize> {
    pub bytes: [u8; CAPACITY],
    pub len: usize,
    pub write_count: usize,
}

impl<const CAPACITY: usize> Preamble<CAPACITY> {
    pub fn push<const LEN: usize>(&mut self, command: [u8; LEN]) {
        self.bytes[self.len..self.len + LEN].copy_from_slice(&command);
        self.len += LEN;
        self.write_count += 1;
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
