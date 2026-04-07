use std::path::{Path, PathBuf};

use crate::settings::AudioRecordingFormat;

use super::MidiApuSnapshot;

pub(crate) struct AudioRecorder;

impl AudioRecorder {
    pub(crate) fn start(
        _path: &Path,
        _sample_rate: u32,
        _format: AudioRecordingFormat,
    ) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "audio recording not supported on web",
        ))
    }

    pub(crate) fn write_samples(&mut self, _samples: &[f32]) {}

    pub(crate) fn write_apu_snapshot(&mut self, _snapshot: MidiApuSnapshot) {}

    pub(crate) fn is_midi(&self) -> bool {
        false
    }

    pub(crate) fn finish(self) -> std::io::Result<PathBuf> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "not available on web",
        ))
    }
}
