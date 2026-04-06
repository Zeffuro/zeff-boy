use zeff_gb_core::hardware::apu::ApuChannelSnapshot as GbApuChannelSnapshot;
use zeff_nes_core::hardware::apu::ApuChannelSnapshot as NesApuChannelSnapshot;
use std::path::{Path, PathBuf};
use crate::settings::AudioRecordingFormat;

pub(crate) fn ogg_vorbis_supported() -> bool {
    false
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum MidiApuSnapshot {
    Gb(GbApuChannelSnapshot),
    Nes(NesApuChannelSnapshot),
}

pub(crate) struct AudioRecorder;

impl AudioRecorder {
    pub(crate) fn start(_path: &Path, _sample_rate: u32, _format: AudioRecordingFormat) -> anyhow::Result<Self> {
        anyhow::bail!("audio recording not supported on web")
    }

    pub(crate) fn write_samples(&mut self, _samples: &[f32]) {}

    pub(crate) fn write_apu_snapshot(&mut self, _snapshot: MidiApuSnapshot) {}

    pub(crate) fn is_midi(&self) -> bool {
        false
    }

    pub(crate) fn finish(self) -> std::io::Result<PathBuf> {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "not available on web"))
    }
}

