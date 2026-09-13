use anyhow::{Result, bail};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BankSelect {
    #[default]
    Gs,
    Mma,
}

impl BankSelect {
    pub const ALL: [Self; 2] = [Self::Gs, Self::Mma];

    #[cfg(test)]
    pub fn id(self) -> &'static str {
        match self {
            Self::Gs => "gs",
            Self::Mma => "mma",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Gs => "GS (CC 0 only)",
            Self::Mma => "MMA (CC 0 + CC 32)",
        }
    }

    pub fn parse(id: &str) -> Result<Self> {
        match id {
            "gs" => Ok(Self::Gs),
            "mma" => Ok(Self::Mma),
            _ => bail!("MIDI bank select must be gs or mma"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioFormat {
    #[default]
    Wav,
    Flac,
    Ogg,
}

impl AudioFormat {
    pub const ALL: [Self; 3] = [Self::Wav, Self::Flac, Self::Ogg];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
            Self::Ogg => "Ogg Vorbis",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SongFormat {
    #[default]
    Midi,
    SoundFont,
    Dls,
    Sfz,
    MidiSoundFont,
    Xm,
    Mod,
    S3m,
    It,
    Vgm,
    Vgz,
    Gbs,
    Nsf,
    Sgc,
    TrackerPack,
    Gsf,
    MiniGsfPack,
    Audio(AudioFormat),
    MappedAssets,
}

pub struct FormatInfo {
    pub format: SongFormat,
    pub id: &'static str,
    pub label: &'static str,
    pub extension: &'static str,
    pub filename: &'static str,
    pub description: &'static str,
}

pub const SONG_FORMATS: &[FormatInfo] = &[
    FormatInfo {
        format: SongFormat::Midi,
        id: "midi",
        label: "MIDI sequence",
        extension: "mid",
        filename: "song.mid",
        description: "Editable sequence. Load the matching instrument bank for the game's sounds.",
    },
    FormatInfo {
        format: SongFormat::SoundFont,
        id: "sf2",
        label: "SoundFont bank",
        extension: "sf2",
        filename: "song.sf2",
        description: "Referenced instruments and keys in SoundFont 2 format.",
    },
    FormatInfo {
        format: SongFormat::Dls,
        id: "dls",
        label: "DLS instrument bank",
        extension: "dls",
        filename: "song.dls",
        description: "Referenced instruments in Downloadable Sounds format.",
    },
    FormatInfo {
        format: SongFormat::Sfz,
        id: "sfz",
        label: "SFZ instrument pack",
        extension: "zip",
        filename: "song-sfz.zip",
        description: "ZIP containing one SFZ per program, shared sample WAVs and a mapping manifest.",
    },
    FormatInfo {
        format: SongFormat::MidiSoundFont,
        id: "midi-sf2",
        label: "MIDI + SoundFont pack",
        extension: "zip",
        filename: "song-midi-sf2.zip",
        description: "Matching song.mid and song.sf2 together, with source metadata.",
    },
    FormatInfo {
        format: SongFormat::Audio(AudioFormat::Wav),
        id: "wav",
        label: "WAV audio",
        extension: "wav",
        filename: "song.wav",
        description: "Approximate stereo render as uncompressed 16-bit PCM.",
    },
    FormatInfo {
        format: SongFormat::Audio(AudioFormat::Flac),
        id: "flac",
        label: "FLAC audio",
        extension: "flac",
        filename: "song.flac",
        description: "The same approximate render with lossless compression.",
    },
    FormatInfo {
        format: SongFormat::Audio(AudioFormat::Ogg),
        id: "ogg",
        label: "Ogg Vorbis audio",
        extension: "ogg",
        filename: "song.ogg",
        description: "The same approximate render with lossy Vorbis compression.",
    },
    FormatInfo {
        format: SongFormat::Xm,
        id: "xm",
        label: "XM tracker module",
        extension: "xm",
        filename: "song.xm",
        description: "Preserve an embedded XM, or convert supported tracker-engine songs to XM.",
    },
    FormatInfo {
        format: SongFormat::Mod,
        id: "mod",
        label: "Original MOD module",
        extension: "mod",
        filename: "song.mod",
        description: "Preserve the validated embedded MOD with its original patterns and sample data.",
    },
    FormatInfo {
        format: SongFormat::S3m,
        id: "s3m",
        label: "Original S3M module",
        extension: "s3m",
        filename: "song.s3m",
        description: "Preserve the validated Scream Tracker 3 module and its original sample data.",
    },
    FormatInfo {
        format: SongFormat::It,
        id: "it",
        label: "Original IT module",
        extension: "it",
        filename: "song.it",
        description: "Preserve the validated Impulse Tracker module and its original sample data.",
    },
    FormatInfo {
        format: SongFormat::TrackerPack,
        id: "tracker",
        label: "Tracker module pack",
        extension: "zip",
        filename: "song-tracker.zip",
        description: "A tracker module together with source identity, conversion details and warnings.",
    },
    FormatInfo {
        format: SongFormat::Vgm,
        id: "vgm",
        label: "VGM register log",
        extension: "vgm",
        filename: "song.vgm",
        description: "Preserve the complete uncompressed VGM log, including trailing source bytes.",
    },
    FormatInfo {
        format: SongFormat::Vgz,
        id: "vgz",
        label: "Original VGZ file",
        extension: "vgz",
        filename: "song.vgz",
        description: "Preserve the original gzip-compressed VGM source byte for byte.",
    },
    FormatInfo {
        format: SongFormat::Gbs,
        id: "gbs",
        label: "GBS driver playback",
        extension: "gbs",
        filename: "song.gbs",
        description: "Export a qualified Game Boy driver and selected song, or preserve a complete imported GBS file.",
    },
    FormatInfo {
        format: SongFormat::Nsf,
        id: "nsf",
        label: "NSF driver playback",
        extension: "nsf",
        filename: "song.nsf",
        description: "Export a qualified NES driver and selected song, or preserve a complete imported NSF file.",
    },
    FormatInfo {
        format: SongFormat::Sgc,
        id: "sgc",
        label: "SGC driver playback",
        extension: "sgc",
        filename: "song.sgc",
        description: "Original Sega sound driver and selected song for an SGC player.",
    },
    FormatInfo {
        format: SongFormat::Gsf,
        id: "gsf",
        label: "GSF driver playback",
        extension: "gsf",
        filename: "song.gsf",
        description: "A qualified original GBA driver plays the selected song in a GSF player. Retains the cartridge data.",
    },
    FormatInfo {
        format: SongFormat::MiniGsfPack,
        id: "minigsf",
        label: "miniGSF + library pack",
        extension: "zip",
        filename: "song-minigsf.zip",
        description: "ZIP containing the selected miniGSF, its shared GSFlib and source metadata. Extract together for playback.",
    },
    FormatInfo {
        format: SongFormat::MappedAssets,
        id: "assets",
        label: "Original mapped assets",
        extension: "zip",
        filename: "song-assets.zip",
        description: "Exact mapped source bytes and provenance; this archive does not play by itself.",
    },
];

impl SongFormat {
    pub fn is_gsf(self) -> bool {
        matches!(self, Self::Gsf | Self::MiniGsfPack)
    }

    pub fn info(self) -> &'static FormatInfo {
        SONG_FORMATS
            .iter()
            .find(|info| info.format == self)
            .expect("every song format has a descriptor")
    }

    pub fn parse(id: &str) -> Result<Self> {
        let id = match id {
            "mid" => "midi",
            "wave" => "wav",
            "vorbis" => "ogg",
            other => other,
        };
        if let Some(info) = SONG_FORMATS.iter().find(|info| info.id == id) {
            return Ok(info.format);
        }
        bail!(
            "audio export format must be {}",
            SONG_FORMATS
                .iter()
                .map(|info| info.id)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_and_ui_share_one_unique_format_registry() {
        let mut ids = std::collections::HashSet::new();
        for info in SONG_FORMATS {
            assert!(ids.insert(info.id));
            assert_eq!(info.format.info().id, info.id);
            assert!(info.filename.ends_with(&format!(".{}", info.extension)));
            assert_eq!(SongFormat::parse(info.id).unwrap(), info.format);
        }
        assert_eq!(SongFormat::parse("mid").unwrap(), SongFormat::Midi);
        assert_eq!(SongFormat::parse("gsf").unwrap(), SongFormat::Gsf);
        assert_eq!(
            SongFormat::parse("minigsf").unwrap(),
            SongFormat::MiniGsfPack
        );
    }

    #[test]
    fn bank_selects_have_stable_cli_ids_and_gs_default() {
        assert_eq!(BankSelect::default(), BankSelect::Gs);
        assert_eq!(BankSelect::parse("gs").unwrap(), BankSelect::Gs);
        assert_eq!(BankSelect::parse("mma").unwrap(), BankSelect::Mma);
        assert!(BankSelect::parse("gm").is_err());
        assert_eq!(BankSelect::Gs.id(), "gs");
        assert_eq!(BankSelect::Mma.id(), "mma");
    }
}
