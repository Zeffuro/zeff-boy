use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail, ensure};
use zeff_emu_common::audio_trace::{
    AudioTraceSource, AudioTraceStart, AudioTraceTiming, WonderSwanAudioTrace,
    WonderSwanResetState, WonderSwanTraceOrigin, WonderSwanTraceWrite,
};
use zeff_ws_core::hardware::apu::Apu;

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::render::RenderOptions;

const MASTER_CLOCK_HZ: u32 = 3_072_000;
const WAVE_RAM_BYTES: usize = 0x4000;
const MAX_ADVANCE_CYCLES: u64 = MASTER_CLOCK_HZ as u64;

fn validate(trace: &WonderSwanAudioTrace, cancel: &AtomicBool) -> Result<()> {
    check_cancel(cancel)?;
    trace.validate_complete()?;
    ensure!(
        trace.start == AudioTraceStart::Reset
            && trace.timing == AudioTraceTiming::BusServiceBoundary
            && trace.cycle_hz == MASTER_CLOCK_HZ
            && trace.cycle_hz_denominator == 1
            && trace.chip.clock_hz == MASTER_CLOCK_HZ
            && trace.chip.reset == WonderSwanResetState::default(),
        "WonderSwan replay requires the canonical native reset and clock contract"
    );
    for event in &trace.events {
        check_cancel(cancel)?;
        ensure!(
            event.pc <= 0x0f_ffff,
            "WonderSwan trace event PC is outside the native address space"
        );
        let origin = match event.write {
            WonderSwanTraceWrite::Register { origin, .. }
            | WonderSwanTraceWrite::WaveRam { origin, .. } => origin,
        };
        ensure!(
            !matches!(
                origin,
                WonderSwanTraceOrigin::CpuInterrupt | WonderSwanTraceOrigin::SoundDma
            ) || (event.pc == 0 && event.instruction_source == AudioTraceSource::Unknown),
            "WonderSwan interrupt and Sound DMA events require unknown provenance"
        );
        match event.write {
            WonderSwanTraceWrite::Register {
                port,
                value,
                origin,
            } => {
                if origin == WonderSwanTraceOrigin::SoundDma && port == 0x69 {
                    bail!("WonderSwan Sound DMA targeted HyperVoice cannot replay as ordinary PSG");
                }
                if (0x64..=0x6b).contains(&port) {
                    bail!("WonderSwan HyperVoice I/O cannot replay as ordinary PSG");
                }
                ensure!(
                    (0x80..=0x95).contains(&port),
                    "WonderSwan trace register write is outside the ordinary sound window"
                );
                ensure!(
                    origin != WonderSwanTraceOrigin::SoundDma || port == 0x89,
                    "WonderSwan Sound DMA write does not target the ordinary voice-volume port"
                );
                ensure!(
                    origin != WonderSwanTraceOrigin::SoundDma || trace.chip.color,
                    "WonderSwan Sound DMA cannot produce an event on a monochrome model"
                );
                ensure!(
                    origin != WonderSwanTraceOrigin::GeneralDma,
                    "WonderSwan General DMA cannot produce an ordinary sound-register write"
                );
                ensure!(
                    port != 0x95 || value & 0x02 == 0,
                    "WonderSwan sound-test fast-sweep mode cannot replay as ordinary PSG"
                );
            }
            WonderSwanTraceWrite::WaveRam {
                address, origin, ..
            } => {
                ensure!(
                    usize::from(address) < WAVE_RAM_BYTES,
                    "WonderSwan trace wave-RAM write is outside the native window"
                );
                ensure!(
                    origin != WonderSwanTraceOrigin::SoundDma,
                    "WonderSwan Sound DMA cannot produce a wave-RAM write"
                );
                ensure!(
                    origin != WonderSwanTraceOrigin::GeneralDma || trace.chip.color,
                    "WonderSwan General DMA cannot produce an event on a monochrome model"
                );
            }
        }
    }
    Ok(())
}

fn source_frames(trace: &WonderSwanAudioTrace, sample_rate: u32) -> Result<usize> {
    ((u128::from(trace.end_cycle) * u128::from(sample_rate)) / u128::from(MASTER_CLOCK_HZ))
        .try_into()
        .context("WonderSwan source duration overflows")
}

fn cycle_for_frames(frames: usize, sample_rate: u32) -> u64 {
    ((frames as u128) * u128::from(MASTER_CLOCK_HZ))
        .div_ceil(u128::from(sample_rate))
        .min(u128::from(u64::MAX)) as u64
}

struct Playback {
    apu: Apu,
    ram: Vec<u8>,
    cycle: u64,
    next_event: usize,
    pending: Vec<f32>,
    scratch: Vec<f32>,
}

impl Playback {
    fn new(trace: &WonderSwanAudioTrace, sample_rate: u32) -> Self {
        Self {
            apu: Apu::new(sample_rate),
            ram: trace.chip.reset.wave_ram.clone(),
            cycle: 0,
            next_event: 0,
            pending: Vec::with_capacity(2048),
            scratch: Vec::with_capacity(2048),
        }
    }

    fn apply_writes(&mut self, trace: &WonderSwanAudioTrace) {
        while let Some(event) = trace.events.get(self.next_event)
            && event.cycle == self.cycle
        {
            match event.write {
                WonderSwanTraceWrite::Register { port, value, .. } => self.apu.write8(port, value),
                WonderSwanTraceWrite::WaveRam { address, value, .. } => {
                    self.ram[usize::from(address)] = value;
                }
            }
            self.next_event += 1;
        }
    }

    fn advance_to(
        &mut self,
        trace: &WonderSwanAudioTrace,
        target: u64,
        cancel: &AtomicBool,
    ) -> Result<()> {
        while self.cycle < target {
            check_cancel(cancel)?;
            self.apply_writes(trace);
            let next_event = trace
                .events
                .get(self.next_event)
                .map_or(target, |event| event.cycle.min(target));
            let step = (next_event - self.cycle).min(MAX_ADVANCE_CYCLES);
            ensure!(step > 0, "WonderSwan trace has a stalled event schedule");
            self.apu.step_cycles(step as u32, &self.ram);
            self.cycle += step;
            self.scratch.clear();
            self.apu.drain_audio_samples_into(&mut self.scratch);
            ensure!(
                self.scratch.len().is_multiple_of(2)
                    && self.scratch.iter().all(|sample| sample.is_finite()),
                "WonderSwan APU returned an invalid stereo interval"
            );
            self.pending.extend_from_slice(&self.scratch);
        }
        self.apply_writes(trace);
        Ok(())
    }
}

pub(crate) struct WonderSwanTraceSession {
    trace: WonderSwanAudioTrace,
    playback: Playback,
    options: RenderOptions,
    duration: usize,
    source_complete: bool,
    position: usize,
    mask: u16,
    interrupted: bool,
    floats: Vec<f32>,
    warnings: Vec<String>,
}

impl WonderSwanTraceSession {
    pub(crate) fn new(
        trace: WonderSwanAudioTrace,
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        validate_options(options)?;
        validate(&trace, cancel)?;
        let max_frames = usize::from(options.max_seconds)
            .checked_mul(options.sample_rate as usize)
            .context("WonderSwan render duration overflows")?;
        let source_frames = source_frames(&trace, options.sample_rate)?;
        let source_complete = source_frames <= max_frames;
        let duration = source_frames.min(max_frames);
        ensure!(
            duration > 0,
            "WonderSwan trace is shorter than one output frame"
        );
        let mut warnings = vec![
            "Replays the captured native WonderSwan PSG interval using its original bus-service timing. This interval may contain effects and silence; it does not identify a song or loop.".into(),
        ];
        if options.fade_seconds > 0 {
            warnings
                .push("The requested fade is applied at the end of the rendered interval.".into());
        }
        Ok(Self {
            playback: Playback::new(&trace, options.sample_rate),
            trace,
            options,
            duration,
            source_complete,
            position: 0,
            mask: 1,
            interrupted: false,
            floats: Vec::with_capacity(2048),
            warnings,
        })
    }

    fn generate(&mut self, frames: usize, cancel: &AtomicBool) -> Result<()> {
        while self.playback.pending.len() / 2 < frames && self.playback.cycle < self.trace.end_cycle
        {
            let target = cycle_for_frames(self.position + frames, self.options.sample_rate)
                .min(self.trace.end_cycle);
            ensure!(
                target > self.playback.cycle,
                "WonderSwan trace ended before its measured native audio interval"
            );
            self.playback.advance_to(&self.trace, target, cancel)?;
        }
        ensure!(
            self.playback.pending.len() >= frames * 2,
            "WonderSwan trace ended before its measured native audio interval"
        );
        self.floats.clear();
        self.floats
            .extend(self.playback.pending.drain(..frames * 2));
        Ok(())
    }
}

impl PcmSession for WonderSwanTraceSession {
    fn has_source_duration_limit(&self) -> bool {
        self.source_complete
    }

    fn duration_frames(&self) -> usize {
        self.duration
    }

    fn position_frames(&self) -> usize {
        self.position
    }

    fn sample_rate(&self) -> u32 {
        self.options.sample_rate
    }

    fn track_count(&self) -> usize {
        1
    }

    fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn reset(&mut self) -> Result<()> {
        self.playback = Playback::new(&self.trace, self.options.sample_rate);
        self.position = 0;
        self.interrupted = false;
        self.floats.clear();
        Ok(())
    }

    fn set_track_mask(&mut self, mask: u16) -> Result<()> {
        ensure!(mask & !1 == 0, "track mask selects an unavailable track");
        self.mask = mask;
        Ok(())
    }

    fn read(&mut self, output: &mut [i16], cancel: &AtomicBool) -> Result<usize> {
        ensure!(
            output.len().is_multiple_of(2),
            "audio buffer must hold complete stereo frames"
        );
        check_cancel(cancel)?;
        ensure!(
            !self.interrupted,
            "reset WonderSwan replay after an interrupted read"
        );
        let frames = (output.len() / 2)
            .min(self.duration - self.position)
            .min(1024);
        if frames == 0 {
            return Ok(0);
        }
        self.interrupted = true;
        self.generate(frames, cancel)?;
        for (index, value) in self.floats.iter().enumerate() {
            let sample = if self.mask == 0 {
                0
            } else {
                (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
            };
            output[index] = fade(
                sample,
                self.position + index / 2,
                self.duration,
                self.options.sample_rate,
                self.options.fade_seconds,
            );
        }
        self.position += frames;
        self.interrupted = false;
        Ok(frames * 2)
    }
}

#[cfg(test)]
mod tests;
