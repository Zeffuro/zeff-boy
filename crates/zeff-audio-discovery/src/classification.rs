use serde::{Deserialize, Serialize};

use crate::catalog::SongRef;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioRole {
    Music,
    Fanfare,
    SoundEffect,
    #[default]
    Unknown,
    Control,
}

impl AudioRole {
    pub const ALL: [Self; 5] = [
        Self::Music,
        Self::Fanfare,
        Self::SoundEffect,
        Self::Unknown,
        Self::Control,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Music => "Music",
            Self::Fanfare => "Fanfare / jingle",
            Self::SoundEffect => "Sound effect",
            Self::Unknown => "Unknown",
            Self::Control => "Control / setup / stop",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleSource {
    Unknown,
    DriverContract,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AudioClassification {
    pub role: AudioRole,
    pub source: RoleSource,
    pub reason: String,
}

impl AudioClassification {
    pub fn driver(role: AudioRole, reason: impl Into<String>) -> Self {
        Self {
            role,
            source: RoleSource::DriverContract,
            reason: reason.into(),
        }
    }

    pub fn assigned(role: AudioRole) -> Self {
        Self {
            role,
            source: RoleSource::User,
            reason: "Manually assigned for this source entry.".into(),
        }
    }
}

impl SongRef<'_> {
    pub fn classification(self) -> AudioClassification {
        match self {
            Self::Radriver(song) => match song.kind {
                crate::radriver::RadriverSongKind::Effect => AudioClassification::driver(AudioRole::SoundEffect, "The qualified driver selects this entry from its effect bank."),
                crate::radriver::RadriverSongKind::CompressedMusic => AudioClassification::driver(AudioRole::Music, "The qualified driver selects this entry through its compressed music sequencer."),
            },
            Self::WsTose(song) if song.profile == "ws-tose-eight-slot-v2" && (1..=28).contains(&song.index) && song.index != 26 => AudioClassification::driver(AudioRole::Music, "The qualified caller routes this selector through the dedicated music wrapper."),
            Self::Natsume(song) => match song.kind {
                crate::natsume::NatsumeSongKind::Music => AudioClassification::driver(AudioRole::Music, "The qualified Natsume descriptor identifies a music entry."),
                crate::natsume::NatsumeSongKind::Setup => AudioClassification::driver(AudioRole::Control, "The driver descriptor configures playback state."),
                crate::natsume::NatsumeSongKind::Silence => AudioClassification::driver(AudioRole::Control, "The driver descriptor is a silence control."),
            },
            Self::Gb(song) if song.index == 0 => AudioClassification::driver(AudioRole::Control, "Selector zero stops the qualified banked driver."),
            Self::Gb(_) => AudioClassification::driver(AudioRole::Music, "Entry belongs to the qualified banked driver's music table."),
            Self::GbQuickThunder(_) => AudioClassification::driver(AudioRole::Music, "Validated QuickThunder music-table entry; the separate sound-effect table is not included."),
            Self::GbSoundSystem(_) => AudioClassification::driver(AudioRole::Music, "The qualified order/instrument table is selected through the music entry point; queued effects use a separate entry point."),
            _ => AudioClassification { role: AudioRole::Unknown, source: RoleSource::Unknown, reason: "No qualified role evidence. Duration and channel count do not establish a role.".into() },
        }
    }

    pub fn classification_key(self) -> String {
        let offset = self.span().map(|s| s.effective_offset);
        let selector = match self {
            Self::Krawall(s) => Some(u32::from(s.subsong)),
            Self::EngineSoftware(s) => Some(u32::from(s.index)),
            Self::Gb(s) => Some(u32::from(s.index)),
            Self::Nes(s) => Some(u32::from(s.index)),
            Self::Natsume(s) => Some(u32::from(s.index)),
            Self::GbNative(s) => Some(u32::from(s.raw_index)),
            Self::NesNative(s) => Some(u32::from(s.raw_index)),
            Self::GaxNative(s) => Some(u32::from(s.index)),
            Self::Musyx(s) => Some(u32::from(s.index)),
            Self::Aas(s) => Some(u32::from(s.index)),
            Self::DescriptorMidi(s) => Some(u32::from(s.index)),
            Self::Nsq(s) => Some(u32::from(s.index)),
            Self::Radriver(s) => Some(u32::from(s.index)),
            Self::Gbass(s) => Some(u32::from(s.index)),
            Self::AasStream(s) => Some(u32::from(s.index)),
            Self::AasPcm(s) => Some(u32::from(s.index)),
            Self::GbMusyx(s) => Some(u32::from(s.index)),
            Self::GbTose(s) => Some(u32::from(s.index)),
            Self::GbQuickThunder(s) => Some(u32::from(s.index)),
            Self::GbGhx(s) => Some(u32::from(s.index)),
            Self::GbSoundSystem(s) => Some(u32::from(s.index)),
            Self::GbCarillon(s) => Some(u32::from(s.index)),
            Self::WsTose(s) => Some(u32::from(s.index)),
            Self::NesTose(s) => Some(u32::from(s.index)),
            Self::SegaPsg(s) => Some(u32::from(s.index)),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Cdda(s) => Some(u32::from(s.number)),
            _ => None,
        };
        format!("{}:{offset:?}:{selector:?}", self.detector_id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsongs_sharing_a_header_have_distinct_stable_role_keys() {
        use crate::krawall::{KrawallNativeProfile, KrawallSong, NativeEntry, PatternEncoding};
        let span = crate::RomSpan {
            effective_offset: 0x100,
            byte_len: 4,
            canonical_cpu_address: 0x0800_0100,
        };
        let entry = NativeEntry {
            source: span,
            cpu_address: span.canonical_cpu_address,
        };
        let first = KrawallSong {
            profile: "test",
            index: 0,
            subsong: 0,
            title: "Test".into(),
            header: span,
            channels: 1,
            start_order: 0,
            order_count: 1,
            pattern_count: 1,
            note_count: 1,
            instrument_count: 0,
            sample_count: 0,
            initial_speed: 6,
            initial_bpm: 125,
            native: KrawallNativeProfile {
                encoding: PatternEncoding::Packed2003,
                process_row: span,
                instrument_bank: 0,
                sample_bank: 0,
                init: entry,
                play: entry,
                instrument_update: entry,
                mixer: entry,
                timer1_irq: entry,
                ram_copies: vec![],
                setup_spans: vec![],
            },
            mapped_spans: vec![],
            warnings: vec![],
        };
        let mut second = first.clone();
        second.subsong = 1;
        assert_ne!(
            SongRef::Krawall(&first).classification_key(),
            SongRef::Krawall(&second).classification_key()
        );
        second.subsong = 0;
        second.index = 99;
        assert_eq!(
            SongRef::Krawall(&first).classification_key(),
            SongRef::Krawall(&second).classification_key()
        );
    }

    #[test]
    fn role_uses_driver_evidence_and_does_not_infer_from_title_or_duration() {
        let song = &crate::gb_quickthunder::GbQuickThunderSong {
            profile: "test",
            index: 1,
            title: "Test".into(),
            bank: 1,
            hardware: crate::gb_quickthunder::GbQuickThunderHardware::CgbDouble,
            table_entry: crate::RomSpan {
                effective_offset: 0x4000,
                byte_len: 2,
                canonical_cpu_address: 0x4000,
            },
            tracks: vec![],
            mapped_spans: vec![],
            warnings: vec![],
        };
        let mut renamed = song.clone();
        renamed.title = "Short sound effect".into();
        assert_eq!(
            SongRef::GbQuickThunder(song).classification(),
            SongRef::GbQuickThunder(&renamed).classification()
        );
        assert_eq!(
            SongRef::GbQuickThunder(song).classification_key(),
            SongRef::GbQuickThunder(&renamed).classification_key()
        );
        assert_eq!(
            SongRef::GbQuickThunder(song).classification().role,
            AudioRole::Music
        );
        for role in AudioRole::ALL {
            assert_eq!(
                serde_json::from_value::<AudioRole>(serde_json::to_value(role).unwrap()).unwrap(),
                role
            );
        }
    }
}
