use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AudioBufferPolicy {
    #[default]
    Auto,
    LowLatency,
    Balanced,
    Stable,
}

impl crate::debug::ui_helpers::EnumLabel for AudioBufferPolicy {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (current 200 ms queue)",
            Self::LowLatency => "Low latency (75 ms queue)",
            Self::Balanced => "Balanced (150 ms queue)",
            Self::Stable => "Stable (300 ms queue)",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::Auto, Self::LowLatency, Self::Balanced, Self::Stable]
    }
}

impl AudioBufferPolicy {
    pub(crate) const fn normal_queue_ms(self) -> usize {
        match self {
            Self::Auto => 200,
            Self::LowLatency => 75,
            Self::Balanced => 150,
            Self::Stable => 300,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum AudioRecordingFormat {
    #[default]
    Wav16,
    WavFloat,
    OggVorbis,
    Midi,
    ZeffEvents,
}

impl crate::debug::ui_helpers::EnumLabel for AudioRecordingFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Wav16 => "WAV 16-bit PCM",
            Self::WavFloat => "WAV 32-bit Float",
            Self::OggVorbis => "OGG Vorbis",
            Self::Midi => "MIDI (APU channels)",
            Self::ZeffEvents => "Zeff audio events",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[
            Self::Wav16,
            Self::WavFloat,
            Self::OggVorbis,
            Self::Midi,
            Self::ZeffEvents,
        ]
    }
}

impl AudioRecordingFormat {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Wav16 | Self::WavFloat => "wav",
            Self::OggVorbis => "ogg",
            Self::Midi => "mid",
            Self::ZeffEvents => "zaudio",
        }
    }

    pub(crate) fn captures_semantics(self) -> bool {
        matches!(self, Self::Midi | Self::ZeffEvents)
    }

    pub(crate) fn supports_uncapped_recording(self) -> bool {
        matches!(self, Self::ZeffEvents)
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioBufferPolicy, AudioRecordingFormat};
    use crate::debug::ui_helpers::EnumLabel;

    #[test]
    fn zeff_events_has_stable_settings_and_file_identity() {
        let format = AudioRecordingFormat::ZeffEvents;
        assert_eq!(format.label(), "Zeff audio events");
        assert_eq!(format.extension(), "zaudio");
        assert!(format.captures_semantics());
        assert!(format.supports_uncapped_recording());
        assert!(AudioRecordingFormat::all_variants().contains(&format));
        assert_eq!(serde_json::to_string(&format).unwrap(), "\"zeff_events\"");
        assert_eq!(
            serde_json::from_str::<AudioRecordingFormat>("\"zeff_events\"").unwrap(),
            format
        );
    }

    #[test]
    fn default_audio_recording_format_remains_wav16() {
        assert_eq!(AudioRecordingFormat::default(), AudioRecordingFormat::Wav16);
        assert!(!AudioRecordingFormat::Wav16.supports_uncapped_recording());
        assert!(!AudioRecordingFormat::Midi.supports_uncapped_recording());
    }

    #[test]
    fn audio_buffer_policy_serializes_and_keeps_legacy_queue_as_default() {
        assert_eq!(AudioBufferPolicy::default(), AudioBufferPolicy::Auto);
        assert_eq!(AudioBufferPolicy::Auto.normal_queue_ms(), 200);
        assert_eq!(AudioBufferPolicy::LowLatency.normal_queue_ms(), 75);
        assert_eq!(
            serde_json::to_string(&AudioBufferPolicy::LowLatency).unwrap(),
            "\"low_latency\""
        );
        assert_eq!(
            serde_json::from_str::<AudioBufferPolicy>("\"stable\"").unwrap(),
            AudioBufferPolicy::Stable
        );
    }
}
