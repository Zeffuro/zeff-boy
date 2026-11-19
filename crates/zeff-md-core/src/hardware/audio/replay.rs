use anyhow::{Result, ensure};
use zeff_emu_common::audio_trace::{MdTraceChip, MdTraceRegion, MdTraceWrite};

use super::{Audio, constants::MAX_SAMPLE_RATE, trace::trace_chip};
use crate::Region;

pub const MD_AUDIO_REPLAY_MAX_FRAMES: usize = 1024;

pub struct MdAudioReplay {
    audio: Audio,
}

impl MdAudioReplay {
    pub fn new(chip: &MdTraceChip, sample_rate: u32) -> Result<Self> {
        ensure!(
            (1..=MAX_SAMPLE_RATE).contains(&sample_rate),
            "invalid MD replay sample rate"
        );
        let region = match chip.region {
            MdTraceRegion::NtscUs => Region::NtscUs,
            MdTraceRegion::NtscJapan => Region::NtscJapan,
            MdTraceRegion::Pal => Region::Pal,
        };
        ensure!(
            *chip == trace_chip(region),
            "unsupported MD native audio reset or clock contract"
        );
        let mut audio = Audio::new(region);
        audio.set_sample_rate(sample_rate);
        Ok(Self { audio })
    }

    pub fn now(&self) -> u64 {
        self.audio.now
    }

    pub fn sample_rate(&self) -> u32 {
        self.audio.sample_rate()
    }

    pub fn advance_to(&mut self, at: u64, output: &mut Vec<f32>) -> Result<()> {
        ensure!(at >= self.audio.now, "MD replay time moved backwards");
        let elapsed = at - self.audio.now;
        let total = u128::from(self.audio.sample_phase)
            + u128::from(elapsed) * u128::from(self.audio.sample_rate);
        ensure!(
            total / u128::from(self.audio.master_hz) <= MD_AUDIO_REPLAY_MAX_FRAMES as u128,
            "MD replay step exceeds the frame limit"
        );
        self.audio.advance_to(at);
        self.audio.drain_into(output);
        Ok(())
    }

    pub fn write(&mut self, at: u64, write: MdTraceWrite, output: &mut Vec<f32>) -> Result<()> {
        if let MdTraceWrite::Opn2Port { port, .. } = write {
            ensure!(port <= 3, "invalid MD replay OPN2 port");
        }
        // Samples through this tick precede the write, including at sample edges.
        self.advance_to(at, output)?;
        match write {
            MdTraceWrite::Psg { value, .. } => self.audio.write_psg(at, value),
            MdTraceWrite::Opn2Port { port, value, .. } => {
                self.audio.write_opn2(at, port, value);
            }
            MdTraceWrite::Opn2Reset { .. } => self.audio.reset_opn2(at),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
