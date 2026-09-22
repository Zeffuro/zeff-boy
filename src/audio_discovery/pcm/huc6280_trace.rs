use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, ensure};
use zeff_emu_common::audio_trace::{
    AudioTraceStart, AudioTraceTiming, Huc6280AudioTrace, Huc6280ResetState, Huc6280TraceRevision,
};
use zeff_pce_core::hardware::{
    HuC6280Psg, PCE_NTSC_COLORBURST_CLOCK_HZ_DENOMINATOR, PCE_NTSC_COLORBURST_CLOCK_HZ_NUMERATOR,
    PCE_NTSC_MASTER_CLOCK_HZ_DENOMINATOR, PCE_NTSC_MASTER_CLOCK_HZ_NUMERATOR,
    PSG_INTERNAL_MASTER_CLOCK_DIVISOR, PSG_MASTER_CLOCK_DIVISOR, PsgPort, PsgRevision,
};

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::render::RenderOptions;

const MAX_ADVANCE_TICKS: u64 = 196_608;

fn revision(revision: Huc6280TraceRevision) -> PsgRevision {
    match revision {
        Huc6280TraceRevision::HuC6280 => PsgRevision::HuC6280,
        Huc6280TraceRevision::HuC6280A => PsgRevision::HuC6280A,
    }
}

fn new_chip(trace_revision: Huc6280TraceRevision, sample_rate: u32) -> HuC6280Psg {
    let mut chip = HuC6280Psg::with_revision(revision(trace_revision));
    chip.set_sample_rate(sample_rate);
    chip
}

pub(in crate::audio_discovery) fn validate(
    trace: &Huc6280AudioTrace,
    cancel: &AtomicBool,
) -> Result<()> {
    check_cancel(cancel)?;
    trace.validate_complete()?;
    ensure!(
        trace.start == AudioTraceStart::Reset
            && trace.timing == AudioTraceTiming::MemoryWriteCompletion
            && u128::from(trace.cycle_hz) * u128::from(PCE_NTSC_MASTER_CLOCK_HZ_DENOMINATOR)
                == u128::from(PCE_NTSC_MASTER_CLOCK_HZ_NUMERATOR)
                    * u128::from(trace.cycle_hz_denominator)
            && trace.chip.clock_hz_numerator == PCE_NTSC_COLORBURST_CLOCK_HZ_NUMERATOR
            && u64::from(trace.chip.clock_hz_denominator)
                == PCE_NTSC_COLORBURST_CLOCK_HZ_DENOMINATOR
            && u64::from(trace.chip.master_clock_divisor) == PSG_MASTER_CLOCK_DIVISOR
            && u64::from(trace.chip.internal_master_clock_divisor)
                == PSG_INTERNAL_MASTER_CLOCK_DIVISOR
            && trace.chip.reset == Huc6280ResetState::default(),
        "HuC6280 replay requires the canonical native reset and clock contract"
    );
    for event in &trace.events {
        check_cancel(cancel)?;
        ensure!(
            event.pc <= u32::from(u16::MAX)
                && (0x1f_e800..=0x1f_ebff).contains(&event.write.physical_address)
                && event.write.register == event.write.physical_address as u8 & 15,
            "HuC6280 trace event is outside the native PSG address contract"
        );
    }
    Ok(())
}

fn cycles_for_frames(trace: &Huc6280AudioTrace, frames: usize, sample_rate: u32) -> u64 {
    let numerator = (frames as u128) * u128::from(trace.cycle_hz);
    let denominator = u128::from(sample_rate) * u128::from(trace.cycle_hz_denominator);
    numerator.div_ceil(denominator).min(u128::from(u64::MAX)) as u64
}

struct Playback {
    chip: HuC6280Psg,
    cycle: u64,
    next_event: usize,
    pending: Vec<f32>,
    scratch: Vec<f32>,
}

impl Playback {
    fn new(trace: &Huc6280AudioTrace, sample_rate: u32) -> Self {
        Self {
            chip: new_chip(trace.chip.revision, sample_rate),
            cycle: 0,
            next_event: 0,
            pending: Vec::with_capacity(2048),
            scratch: Vec::with_capacity(2048),
        }
    }

    fn apply_writes(&mut self, trace: &Huc6280AudioTrace) {
        while let Some(event) = trace.events.get(self.next_event)
            && event.cycle == self.cycle
        {
            self.chip.write_port(
                PsgPort::from_offset(event.write.register),
                event.write.value,
            );
            self.next_event += 1;
        }
    }

    fn advance_to(
        &mut self,
        trace: &Huc6280AudioTrace,
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
            let step = (next_event - self.cycle).min(MAX_ADVANCE_TICKS);
            ensure!(step > 0, "HuC6280 trace has a stalled event schedule");
            self.chip.advance_master_ticks(step);
            self.cycle += step;
            self.chip.drain_audio_samples_into(&mut self.scratch);
            ensure!(
                self.scratch.len().is_multiple_of(2)
                    && self.scratch.iter().all(|sample| sample.is_finite()),
                "HuC6280 chip returned an invalid stereo interval"
            );
            self.pending.extend_from_slice(&self.scratch);
        }
        self.apply_writes(trace);
        Ok(())
    }
}

fn source_frames(
    trace: &Huc6280AudioTrace,
    sample_rate: u32,
    cancel: &AtomicBool,
) -> Result<usize> {
    let mut playback = Playback::new(trace, sample_rate);
    let mut frames = 0_usize;
    while playback.cycle < trace.end_cycle {
        let target = trace
            .end_cycle
            .min(playback.cycle.saturating_add(MAX_ADVANCE_TICKS));
        playback.advance_to(trace, target, cancel)?;
        frames = frames
            .checked_add(playback.pending.len() / 2)
            .context("HuC6280 source duration overflows")?;
        playback.pending.clear();
    }
    Ok(frames)
}

pub(crate) struct Huc6280TraceSession {
    trace: Huc6280AudioTrace,
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

impl Huc6280TraceSession {
    pub(crate) fn new(
        trace: Huc6280AudioTrace,
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        validate_options(options)?;
        validate(&trace, cancel)?;
        let max_frames = usize::from(options.max_seconds)
            .checked_mul(options.sample_rate as usize)
            .context("HuC6280 render duration overflows")?;
        let probe_limit = cycles_for_frames(&trace, max_frames, options.sample_rate)
            .saturating_add(MAX_ADVANCE_TICKS);
        let source_complete = trace.end_cycle <= probe_limit;
        let duration = if source_complete {
            source_frames(&trace, options.sample_rate, cancel)?.min(max_frames)
        } else {
            max_frames
        };
        ensure!(
            duration > 0,
            "HuC6280 trace is shorter than one output frame"
        );
        let mut warnings = vec![
            "Replays the captured native HuC6280 PSG interval using its original master-clock timing. This interval may contain effects and silence; it does not identify a song or loop.".into(),
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
            let missing = frames - self.playback.pending.len() / 2;
            let target = self
                .playback
                .cycle
                .saturating_add(cycles_for_frames(
                    &self.trace,
                    missing.max(1),
                    self.options.sample_rate,
                ))
                .min(self.trace.end_cycle);
            self.playback.advance_to(&self.trace, target, cancel)?;
        }
        ensure!(
            self.playback.pending.len() >= frames * 2,
            "HuC6280 trace ended before its measured native audio interval"
        );
        self.floats.clear();
        self.floats
            .extend(self.playback.pending.drain(..frames * 2));
        Ok(())
    }
}

impl PcmSession for Huc6280TraceSession {
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
            "reset HuC6280 replay after an interrupted read"
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
