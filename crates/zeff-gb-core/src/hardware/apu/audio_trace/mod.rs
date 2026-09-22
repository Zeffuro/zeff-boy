mod validate;

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use zeff_emu_common::audio_trace::{GameBoyAudioTrace, GameBoyTraceWrite};

use super::Apu;

pub struct GameBoyTraceReplayer {
    trace: GameBoyAudioTrace,
    apu: Apu,
    sample_rate: u32,
    duration: u64,
    next_event: usize,
    next_drain: usize,
    drain_frames: Vec<u64>,
    interrupted: bool,
}

impl GameBoyTraceReplayer {
    pub fn new(trace: GameBoyAudioTrace, sample_rate: u32) -> Result<Self> {
        let apu = validate::new_apu(&trace.chip, sample_rate)?;
        let drain_frames = validate::schedule(&trace, sample_rate)?;
        let duration = drain_frames.iter().sum();
        Ok(Self {
            trace,
            apu,
            sample_rate,
            duration,
            next_event: 0,
            next_drain: 0,
            drain_frames,
            interrupted: false,
        })
    }

    pub fn duration_frames(&self) -> u64 {
        self.duration
    }

    pub fn reset(&mut self) -> Result<()> {
        self.apu = validate::new_apu(&self.trace.chip, self.sample_rate)?;
        self.next_event = 0;
        self.next_drain = 0;
        self.interrupted = false;
        Ok(())
    }

    pub fn read_next_drain(&mut self, cancel: &AtomicBool) -> Result<Option<Vec<f32>>> {
        ensure!(!cancel.load(Ordering::Relaxed), "audio replay cancelled");
        ensure!(
            !self.interrupted,
            "reset Game Boy replay after an interrupted read"
        );
        self.interrupted = true;
        while let Some(event) = self.trace.events.get(self.next_event) {
            ensure!(!cancel.load(Ordering::Relaxed), "audio replay cancelled");
            self.next_event += 1;
            match event.write {
                GameBoyTraceWrite::NativeBatch {
                    cycles,
                    repetitions,
                } => {
                    for batch in 0..repetitions {
                        if batch.is_multiple_of(1024) {
                            ensure!(!cancel.load(Ordering::Relaxed), "audio replay cancelled");
                        }
                        self.apu.step(u64::from(cycles));
                    }
                }
                GameBoyTraceWrite::Register { address, value, .. } => {
                    self.apu.write(address, value)
                }
                GameBoyTraceWrite::WaveRam {
                    address,
                    value,
                    applied_index,
                    ..
                } => {
                    ensure!(
                        self.apu
                            .wave_ram_cpu_access_index(address)
                            .map(|index| index as u8)
                            == applied_index,
                        "Game Boy wave-RAM access differs at cycle {}",
                        event.cycle
                    );
                    self.apu.write(address, value);
                }
                GameBoyTraceWrite::SequencerClock { primary, secondary } => {
                    for _ in 0..secondary {
                        self.apu.clock_div_apu_secondary_event();
                    }
                    for _ in 0..primary {
                        self.apu.clock_div_apu();
                    }
                }
                GameBoyTraceWrite::NativeDividerPhase { skip_next } => {
                    self.apu.skip_next_div_apu_event_if(skip_next);
                }
                GameBoyTraceWrite::SpeedSwitch { double_speed } => {
                    self.apu.set_cgb_double_speed(double_speed)
                }
                GameBoyTraceWrite::PcmDrain { .. } => {
                    let samples = self.apu.drain_samples();
                    ensure!(
                        samples.len() as u64 == self.drain_frames[self.next_drain] * 2
                            && samples.iter().all(|sample| sample.is_finite()),
                        "Game Boy native output differs at drain {}",
                        self.next_drain
                    );
                    self.next_drain += 1;
                    self.interrupted = false;
                    return Ok(Some(samples));
                }
                GameBoyTraceWrite::DividerReset { .. }
                | GameBoyTraceWrite::Stop { .. }
                | GameBoyTraceWrite::SpeedSwitchDelay { .. } => {}
                GameBoyTraceWrite::NativeOutputChange { .. } => unreachable!("validated schedule"),
            }
        }
        self.interrupted = false;
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
