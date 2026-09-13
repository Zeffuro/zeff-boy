use anyhow::{Result, bail, ensure};
use serde::Serialize;

use super::super::formats::BankSelect;

pub(crate) const DEFAULT_SAMPLE_RATE: u32 = 48_000;
pub(crate) const DEFAULT_MAX_SECONDS: u16 = 30 * 60;
pub(crate) const MAX_DURATION_SECONDS: u16 = 2 * 60 * 60;
pub(crate) const MAX_FADE_SECONDS: u8 = 15;
pub(crate) const MAX_LOOP_PASSES: u8 = 8;
pub(crate) const SAMPLE_RATES: [u32; 4] = [44_100, DEFAULT_SAMPLE_RATE, 63_072, 96_000];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) enum PlaybackGain {
    #[default]
    #[serde(rename = "raw")]
    Raw,
    #[serde(rename = "mp2k")]
    Mp2kAmplitude,
}

impl PlaybackGain {
    pub(crate) const ALL: [Self; 2] = [Self::Raw, Self::Mp2kAmplitude];

    #[cfg(test)]
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Mp2kAmplitude => "mp2k",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Raw => "Raw controls",
            Self::Mp2kAmplitude => "MP2K amplitude",
        }
    }

    pub(crate) fn parse(id: &str) -> Result<Self> {
        match id {
            "raw" => Ok(Self::Raw),
            "mp2k" => Ok(Self::Mp2kAmplitude),
            _ => bail!("playback gain must be raw or mp2k"),
        }
    }

    pub(in crate::audio_discovery) fn map(self, value: u8) -> u8 {
        match self {
            Self::Raw => value,
            // RustySynth squares velocity and CC7; invert that curve for MP2K's linear factors.
            Self::Mp2kAmplitude => {
                let target = 127 * u32::from(value);
                let floor = target.isqrt();
                let rounded = if target > floor * floor + floor {
                    floor + 1
                } else {
                    floor
                };
                rounded as u8
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct RenderOptions {
    pub(crate) sample_rate: u32,
    pub(crate) loops: u8,
    pub(crate) max_seconds: u16,
    pub(crate) fade_seconds: u8,
    pub(crate) skip_channel10: bool,
    pub(crate) bank_select: BankSelect,
    pub(crate) playback_gain: PlaybackGain,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            loops: 1,
            max_seconds: DEFAULT_MAX_SECONDS,
            fade_seconds: 0,
            skip_channel10: true,
            bank_select: BankSelect::default(),
            playback_gain: PlaybackGain::default(),
        }
    }
}

pub(super) fn validate_options(options: RenderOptions) -> Result<()> {
    validate_sample_rate(options.sample_rate)?;
    ensure!(
        (1..=MAX_LOOP_PASSES).contains(&options.loops),
        "loop count must be between 1 and {MAX_LOOP_PASSES}"
    );
    ensure!(
        (1..=MAX_DURATION_SECONDS).contains(&options.max_seconds),
        "maximum duration must be between 1 and {MAX_DURATION_SECONDS} seconds"
    );
    ensure!(
        options.fade_seconds <= MAX_FADE_SECONDS,
        "fade duration must be between 0 and {MAX_FADE_SECONDS} seconds"
    );
    Ok(())
}

pub(crate) fn validate_sample_rate(sample_rate: u32) -> Result<()> {
    ensure!(
        SAMPLE_RATES.contains(&sample_rate),
        "output sample rate must be 44100, 48000, 63072 or 96000 Hz"
    );
    Ok(())
}
