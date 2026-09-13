use std::collections::HashSet;

use anyhow::{Result, ensure};

pub(super) const MAX_PCM_POINTS: usize = 32 * 1024 * 1024;
pub(super) const MAX_ITEMS: usize = 4096;

#[derive(Clone, Debug)]
pub(crate) struct InstrumentBank {
    pub(crate) name: String,
    pub(crate) comment: String,
    pub(crate) samples: Vec<Sample>,
    pub(crate) presets: Vec<Preset>,
}

#[derive(Clone, Debug)]
pub(crate) struct Sample {
    pub(crate) name: String,
    pub(crate) pcm: Vec<i16>,
    /// Exact playback rate at original_pitch, before file-format rounding.
    pub(crate) sample_rate: f64,
    pub(crate) original_pitch: u8,
    /// Zero-based, half-open PCM indices: [start, end).
    pub(crate) loop_range: Option<(u32, u32)>,
}

impl Sample {
    pub(crate) fn integer_rate(&self) -> u32 {
        self.sample_rate.round() as u32
    }

    pub(crate) fn pitch_correction(&self) -> i16 {
        (1200.0 * (self.sample_rate / f64::from(self.integer_rate())).log2()).round() as i16
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Preset {
    pub(crate) name: String,
    /// Bank 128 is the percussion alias of the matching bank-0 program.
    pub(crate) bank: u16,
    pub(crate) program: u16,
    pub(crate) zones: Vec<Region>,
}

#[derive(Clone, Debug)]
pub(crate) struct Region {
    pub(crate) key_start: u8,
    pub(crate) key_end: u8,
    pub(crate) sample_index: usize,
    pub(crate) root_key: u8,
    /// Cents per MIDI key; zero means fixed pitch.
    pub(crate) scale_tuning: i16,
    pub(crate) coarse_tune: i16,
    pub(crate) fine_tune: i16,
    /// -500 is full left, 0 is center, +500 is full right.
    pub(crate) pan: i16,
    pub(crate) attack_seconds: f32,
    /// Actual time from the peak to `sustain_level`.
    pub(crate) decay_seconds: f32,
    pub(crate) sustain_level: f32,
    /// Full-scale time from the peak to the -100 dB floor.
    pub(crate) release_seconds: f32,
}

pub(super) fn validate(bank: &InstrumentBank) -> Result<()> {
    ensure!(
        !bank.name.is_empty() && bank.name.len() <= 1024,
        "invalid instrument bank name"
    );
    ensure!(
        bank.comment.len() <= 1024 * 1024,
        "instrument bank metadata is too large"
    );
    ensure!(
        (1..=MAX_ITEMS).contains(&bank.samples.len()),
        "invalid instrument bank sample count"
    );
    ensure!(
        (1..=MAX_ITEMS).contains(&bank.presets.len()),
        "invalid instrument bank preset count"
    );
    let mut points = 0usize;
    for (index, sample) in bank.samples.iter().enumerate() {
        ensure!(!sample.pcm.is_empty(), "sample {index} has no PCM points");
        ensure!(
            sample.sample_rate.is_finite()
                && (1.0..=192_000.0).contains(&sample.sample_rate.round()),
            "sample {index} has an invalid sample rate"
        );
        ensure!(
            sample.original_pitch <= 127,
            "sample {index} has an invalid original pitch"
        );
        if let Some((start, end)) = sample.loop_range {
            ensure!(
                start < end && end as usize <= sample.pcm.len(),
                "sample {index} has an invalid loop"
            );
        }
        points = points
            .checked_add(sample.pcm.len())
            .filter(|count| *count <= MAX_PCM_POINTS)
            .ok_or_else(|| anyhow::anyhow!("instrument bank exceeds its PCM point limit"))?;
    }
    let mut locations = HashSet::new();
    let mut zones = 0usize;
    for preset in &bank.presets {
        ensure!(
            preset.bank <= 128
                && preset.program <= 127
                && locations.insert((preset.bank, preset.program)),
            "invalid or duplicate instrument bank/program"
        );
        ensure!(!preset.zones.is_empty(), "instrument has no regions");
        zones = zones
            .checked_add(preset.zones.len())
            .filter(|count| *count <= MAX_ITEMS)
            .ok_or_else(|| anyhow::anyhow!("instrument bank exceeds its region limit"))?;
        for region in &preset.zones {
            ensure!(
                region.key_start <= region.key_end
                    && region.key_end <= 127
                    && region.root_key <= 127,
                "invalid instrument key range or root"
            );
            ensure!(
                region.sample_index < bank.samples.len(),
                "instrument region references a missing sample"
            );
            ensure!(
                (0..=1200).contains(&region.scale_tuning)
                    && (-120..=120).contains(&region.coarse_tune)
                    && (-99..=99).contains(&region.fine_tune),
                "invalid instrument tuning"
            );
            ensure!((-500..=500).contains(&region.pan), "invalid instrument pan");
            ensure!(
                [
                    region.attack_seconds,
                    region.decay_seconds,
                    region.release_seconds
                ]
                .iter()
                .all(|value| value.is_finite() && (0.0..=100.0).contains(value)),
                "invalid instrument envelope time"
            );
            ensure!(
                region.sustain_level.is_finite() && (0.0..=1.0).contains(&region.sustain_level),
                "invalid instrument sustain amplitude"
            );
        }
    }
    Ok(())
}
