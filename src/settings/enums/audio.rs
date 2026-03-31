use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum AudioRecordingFormat {
    #[default]
    Wav16,
    WavFloat,
    OggVorbis,
    Midi,
}

impl crate::debug::ui_helpers::EnumLabel for AudioRecordingFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Wav16 => "WAV 16-bit PCM",
            Self::WavFloat => "WAV 32-bit Float",
            Self::OggVorbis => "OGG Vorbis",
            Self::Midi => "MIDI (APU channels)",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::Wav16, Self::WavFloat, Self::OggVorbis, Self::Midi]
    }
}

impl AudioRecordingFormat {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Wav16 | Self::WavFloat => "wav",
            Self::OggVorbis => "ogg",
            Self::Midi => "mid",
        }
    }

    pub(crate) fn is_midi(self) -> bool {
        matches!(self, Self::Midi)
    }
}
