use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};
use zeff_emu_common::audio_trace::{
    AudioTraceSource, AudioTraceStart, AudioTraceTiming, NesAudioTrace, NesTraceOrigin,
    NesTraceWrite,
};
use zeff_nes_core::hardware::apu::Apu;

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::render::RenderOptions;

pub(crate) struct NesTraceSession {
    trace: NesAudioTrace,
    apu: Apu,
    options: RenderOptions,
    source_frames: u64,
    duration: usize,
    position: usize,
    cycle: u64,
    next_event: usize,
    mask: u16,
    interrupted: bool,
    warnings: Vec<String>,
}

fn validate(trace: &NesAudioTrace, cancel: &AtomicBool) -> Result<()> {
    check_cancel(cancel)?;
    trace.validate_complete()?;
    ensure!(
        trace.start == AudioTraceStart::Reset
            && trace.timing == AudioTraceTiming::CpuBusCycleBoundary
            && u64::from(trace.cycle_hz) == trace.chip.clock_hz_numerator
            && trace.cycle_hz_denominator == trace.chip.clock_hz_denominator,
        "NES replay requires the original native APU cycle contract"
    );
    for event in &trace.events {
        check_cancel(cancel)?;
        ensure!(
            event.cycle < trace.end_cycle && event.pc <= u32::from(u16::MAX),
            "NES trace event is outside its native interval or address space"
        );
        match event.write {
            NesTraceWrite::Register {
                address, odd_cycle, ..
            } => {
                ensure!(
                    matches!(address, 0x4000..=0x4013 | 0x4015 | 0x4017)
                        && odd_cycle == event.cycle.is_multiple_of(2),
                    "unsupported NES APU register or bus-cycle parity"
                );
            }
            NesTraceWrite::StatusRead { value, origin } => {
                ensure!(value & 0x20 == 0, "invalid native NES APU status value");
                if origin != NesTraceOrigin::Cpu {
                    ensure!(
                        event.pc == 0 && event.instruction_source == AudioTraceSource::Unknown,
                        "autonomous NES status read has an attributed CPU instruction"
                    );
                }
            }
            NesTraceWrite::DmcFetch {
                address, source, ..
            } => {
                ensure!(
                    address >= 0x8000
                        && matches!(
                            source,
                            AudioTraceSource::CartridgeRom {
                                bit_reversed: false,
                                ..
                            }
                        )
                        && event.pc == 0
                        && event.instruction_source == AudioTraceSource::Unknown,
                    "unsupported NES DMC fetch address or provenance"
                );
            }
        }
    }
    Ok(())
}

impl NesTraceSession {
    pub(crate) fn new(
        trace: NesAudioTrace,
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        validate_options(options)?;
        validate(&trace, cancel)?;
        let mut apu = Apu::new_for_audio_trace(&trace.chip, f64::from(options.sample_rate))?;
        apu.set_debug_collection_enabled(false);
        let source_frames =
            Apu::audio_trace_frame_count(&trace.chip, options.sample_rate, trace.end_cycle)?;
        let duration = source_frames
            .min(u64::from(options.max_seconds) * u64::from(options.sample_rate))
            as usize;
        ensure!(duration > 0, "NES trace is shorter than one output frame");
        let mut warnings = vec![
            "Replays the captured native NES APU interval, including fetched DMC data, effects and silence. This interval does not identify a song or loop.".into(),
        ];
        if options.fade_seconds > 0 {
            warnings
                .push("The requested fade is applied at the end of the rendered interval.".into());
        }
        Ok(Self {
            trace,
            apu,
            options,
            source_frames,
            duration,
            position: 0,
            cycle: 0,
            next_event: 0,
            mask: 1,
            interrupted: false,
            warnings,
        })
    }

    fn generate(&mut self, frames: usize, cancel: &AtomicBool) -> Result<()> {
        self.apu.sample_buffer.clear();
        let finish_source = (self.position + frames) as u64 == self.source_frames;
        while self.cycle < self.trace.end_cycle
            && (self.apu.sample_buffer.len() < frames || finish_source)
        {
            if self.cycle.is_multiple_of(1024) {
                check_cancel(cancel)?;
            }
            while let Some(event) = self.trace.events.get(self.next_event)
                && event.cycle == self.cycle
            {
                check_cancel(cancel)?;
                match event.write {
                    NesTraceWrite::Register {
                        address,
                        value,
                        odd_cycle,
                    } => {
                        self.apu.write_register(address, value, odd_cycle);
                    }
                    NesTraceWrite::StatusRead { value, .. } => {
                        ensure!(
                            self.apu.read_status() == value,
                            "NES APU status differs from the native trace at cycle {}",
                            self.cycle
                        );
                    }
                    NesTraceWrite::DmcFetch { address, value, .. } => {
                        ensure!(
                            self.apu.dmc.needs_dma() && self.apu.dmc.dma_address() == address,
                            "NES DMC fetch differs from the native trace at cycle {}",
                            self.cycle
                        );
                        self.apu.dmc.fill_sample_buffer(value);
                    }
                }
                self.next_event += 1;
            }
            self.apu.tick();
            self.cycle += 1;
        }
        ensure!(
            self.apu.sample_buffer.len() == frames
                && self
                    .apu
                    .sample_buffer
                    .iter()
                    .all(|sample| sample.is_finite()),
            "NES APU returned an invalid native sample interval"
        );
        Ok(())
    }
}

impl PcmSession for NesTraceSession {
    fn has_source_duration_limit(&self) -> bool {
        true
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
        self.apu = Apu::new_for_audio_trace(&self.trace.chip, f64::from(self.options.sample_rate))?;
        self.apu.set_debug_collection_enabled(false);
        self.position = 0;
        self.cycle = 0;
        self.next_event = 0;
        self.interrupted = false;
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
            "reset NES replay after an interrupted read"
        );
        let frames = (output.len() / 2)
            .min(self.duration - self.position)
            .min(1024);
        if frames == 0 {
            return Ok(0);
        }
        self.interrupted = true;
        self.generate(frames, cancel)?;
        for (index, value) in self.apu.sample_buffer.iter().enumerate() {
            let sample = if self.mask == 0 {
                0
            } else {
                (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
            };
            let sample = fade(
                sample,
                self.position + index,
                self.duration,
                self.options.sample_rate,
                self.options.fade_seconds,
            );
            output[index * 2..index * 2 + 2].fill(sample);
        }
        self.position += frames;
        self.interrupted = false;
        Ok(frames * 2)
    }
}

#[cfg(test)]
mod tests;
