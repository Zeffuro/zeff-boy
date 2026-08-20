use std::path::{Path, PathBuf};

use crate::settings::AudioRecordingFormat;

use crate::audio_tooling::{AudioRecordingContext, AudioSemanticFrame};

use super::AudioTimelineDiscontinuity;

pub(crate) struct AudioRecorder;

impl AudioRecorder {
    pub(crate) fn start(
        _path: &Path,
        _sample_rate: u32,
        _format: AudioRecordingFormat,
        _context: Option<AudioRecordingContext>,
    ) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "audio recording not supported on web",
        ))
    }

    pub(crate) fn write_samples(&mut self, _samples: &[f32]) {}

    pub(crate) fn write_audio_semantic_frame(&mut self, _frame: AudioSemanticFrame) {}

    pub(crate) fn begin_semantic_timeline_epoch(&mut self, _reason: AudioTimelineDiscontinuity) {}

    pub(crate) fn captures_semantics(&self) -> bool {
        false
    }

    pub(crate) fn supports_uncapped_recording(&self) -> bool {
        false
    }

    pub(crate) fn finish(self) -> std::io::Result<PathBuf> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "not available on web",
        ))
    }
}
