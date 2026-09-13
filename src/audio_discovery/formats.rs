use anyhow::{Result, ensure};

use super::{SampleInventory, render::RenderOptions};
pub(crate) use zeff_audio_discovery::formats::*;

pub(crate) trait FormatAvailability {
    fn available(self) -> bool;
}

impl FormatAvailability for AudioFormat {
    fn available(self) -> bool {
        self != Self::Ogg || cfg!(feature = "audio-recording")
    }
}

impl FormatAvailability for SongFormat {
    fn available(self) -> bool {
        match self {
            Self::Audio(format) => format.available(),
            _ => true,
        }
    }
}

pub(crate) fn parse_song_format(id: &str) -> Result<SongFormat> {
    let format = SongFormat::parse(id)?;
    ensure!(
        format.available(),
        "{} requires the audio-recording build feature",
        format.info().label
    );
    Ok(format)
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ExportKind {
    Song {
        format: SongFormat,
        options: RenderOptions,
    },
    Sample {
        format: AudioFormat,
        sample: SampleInventory,
    },
}

impl ExportKind {
    #[cfg(test)]
    pub(crate) fn song(format: SongFormat) -> Self {
        Self::Song {
            format,
            options: RenderOptions::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vorbis_selection_follows_the_application_build_feature() {
        assert!(AudioFormat::Wav.available());
        assert!(AudioFormat::Flac.available());
        for id in ["ogg", "vorbis"] {
            assert_eq!(
                parse_song_format(id).is_ok(),
                cfg!(feature = "audio-recording")
            );
        }
    }
}
