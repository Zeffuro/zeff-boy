use super::{ScanReport, SongCandidate, SourceSpan, gax::GaxSong, tracker::EmbeddedModule};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(tag = "engine", content = "index", rename_all = "snake_case")]
pub enum SongId {
    Mp2k(usize),
    Gax(usize),
    EngineSoftware(usize),
    Krawall(usize),
    GaxNative(usize),
    Musyx(usize),
    Aas(usize),
    DescriptorMidi(usize),
    Nsq(usize),
    Radriver(usize),
    Gbass(usize),
    AasStream(usize),
    AasPcm(usize),
    Gb(usize),
    Nes(usize),
    NesNative(usize),
    GbNative(usize),
    Huge(usize),
    GbMusyx(usize),
    GbTose(usize),
    #[serde(rename = "gb_quickthunder")]
    GbQuickThunder(usize),
    GbGhx(usize),
    GbSoundSystem(usize),
    GbCarillon(usize),
    WsTose(usize),
    NesTose(usize),
    SegaPsg(usize),
    Natsume(usize),
    Vgm(usize),
    Rip(usize),
    Module(usize),
    #[cfg(not(target_arch = "wasm32"))]
    Cdda(usize),
}

#[derive(Clone, Copy)]
pub enum SongRef<'a> {
    Mp2k(&'a SongCandidate),
    Gax(&'a GaxSong),
    EngineSoftware(&'a super::engine_software::EngineSoftwareSong),
    Krawall(&'a super::krawall::KrawallSong),
    GaxNative(&'a super::gax_native::GaxNativeSong),
    Musyx(&'a super::musyx::MusyxSong),
    Aas(&'a super::aas::AasSong),
    DescriptorMidi(&'a super::descriptor_midi::DescriptorMidiSong),
    Nsq(&'a super::nsq::NsqSong),
    Radriver(&'a super::radriver::RadriverSong),
    Gbass(&'a super::gbass::GbassSong),
    AasStream(&'a super::aas_stream::AasStreamSong),
    AasPcm(&'a super::aas_pcm::AasPcmSong),
    Gb(&'a super::gb_music::GbSong),
    Nes(&'a super::nes_music::NesSong),
    NesNative(&'a super::nes_native::NesNativeSong),
    GbNative(&'a super::gb_native::GbNativeSong),
    Huge(&'a super::huge::catalog::HugeSong),
    GbMusyx(&'a super::gb_musyx::GbMusyxSong),
    GbTose(&'a super::gb_tose::GbToseSong),
    GbQuickThunder(&'a super::gb_quickthunder::GbQuickThunderSong),
    GbGhx(&'a super::gb_ghx::GbGhxSong),
    GbSoundSystem(&'a super::gb_sound_system::GbSoundSystemSong),
    GbCarillon(&'a super::gb_carillon::GbCarillonSong),
    WsTose(&'a super::ws_tose::WsToseSong),
    NesTose(&'a super::nes_tose::NesToseSong),
    SegaPsg(&'a super::sega_psg::SegaPsgSong),
    Natsume(&'a super::natsume::NatsumeSong),
    Vgm(&'a super::vgm::VgmLog),
    Rip(&'a super::rips::MusicRip),
    Module(&'a EmbeddedModule),
    #[cfg(not(target_arch = "wasm32"))]
    Cdda(&'a super::cdda::CdAudioTrack),
}

impl ScanReport {
    pub fn song_count(&self) -> usize {
        self.song_ids().count()
    }

    pub fn song_ids(&self) -> impl Iterator<Item = SongId> {
        let songs = (0..self.candidates.len())
            .map(SongId::Mp2k)
            .chain((0..self.gax_songs.len()).map(SongId::Gax))
            .chain((0..self.engine_software_songs.len()).map(SongId::EngineSoftware))
            .chain((0..self.krawall_songs.len()).map(SongId::Krawall))
            .chain((0..self.gax_native_songs.len()).map(SongId::GaxNative))
            .chain((0..self.musyx_songs.len()).map(SongId::Musyx))
            .chain((0..self.aas_songs.len()).map(SongId::Aas))
            .chain((0..self.descriptor_midi_songs.len()).map(SongId::DescriptorMidi))
            .chain((0..self.nsq_songs.len()).map(SongId::Nsq))
            .chain((0..self.radriver_songs.len()).map(SongId::Radriver))
            .chain((0..self.gbass_songs.len()).map(SongId::Gbass))
            .chain((0..self.aas_stream_songs.len()).map(SongId::AasStream))
            .chain((0..self.aas_pcm_songs.len()).map(SongId::AasPcm))
            .chain((0..self.gb_songs.len()).map(SongId::Gb))
            .chain((0..self.gb_native_songs.len()).map(SongId::GbNative))
            .chain((0..self.huge_songs.len()).map(SongId::Huge))
            .chain((0..self.gb_musyx_songs.len()).map(SongId::GbMusyx))
            .chain((0..self.gb_tose_songs.len()).map(SongId::GbTose))
            .chain((0..self.gb_quickthunder_songs.len()).map(SongId::GbQuickThunder))
            .chain((0..self.gb_ghx_songs.len()).map(SongId::GbGhx))
            .chain((0..self.gb_sound_system_songs.len()).map(SongId::GbSoundSystem))
            .chain((0..self.gb_carillon_songs.len()).map(SongId::GbCarillon))
            .chain((0..self.ws_tose_songs.len()).map(SongId::WsTose))
            .chain((0..self.nes_tose_songs.len()).map(SongId::NesTose))
            .chain((0..self.nes_songs.len()).map(SongId::Nes))
            .chain((0..self.nes_native_songs.len()).map(SongId::NesNative))
            .chain((0..self.sega_psg_songs.len()).map(SongId::SegaPsg))
            .chain((0..self.natsume_songs.len()).map(SongId::Natsume))
            .chain((0..self.vgm_logs.len()).map(SongId::Vgm))
            .chain((0..self.music_rips.len()).map(SongId::Rip))
            .chain((0..self.tracker_modules.len()).map(SongId::Module));
        #[cfg(not(target_arch = "wasm32"))]
        let songs = songs.chain((0..self.cdda_tracks.len()).map(SongId::Cdda));
        songs
    }

    pub fn song(&self, id: SongId) -> Option<SongRef<'_>> {
        match id {
            SongId::Mp2k(index) => self.candidates.get(index).map(SongRef::Mp2k),
            SongId::Gax(index) => self.gax_songs.get(index).map(SongRef::Gax),
            SongId::EngineSoftware(index) => self
                .engine_software_songs
                .get(index)
                .map(SongRef::EngineSoftware),
            SongId::Krawall(index) => self.krawall_songs.get(index).map(SongRef::Krawall),
            SongId::GaxNative(index) => self.gax_native_songs.get(index).map(SongRef::GaxNative),
            SongId::Musyx(index) => self.musyx_songs.get(index).map(SongRef::Musyx),
            SongId::Aas(index) => self.aas_songs.get(index).map(SongRef::Aas),
            SongId::DescriptorMidi(index) => self
                .descriptor_midi_songs
                .get(index)
                .map(SongRef::DescriptorMidi),
            SongId::Nsq(index) => self.nsq_songs.get(index).map(SongRef::Nsq),
            SongId::Radriver(index) => self.radriver_songs.get(index).map(SongRef::Radriver),
            SongId::Gbass(index) => self.gbass_songs.get(index).map(SongRef::Gbass),
            SongId::AasStream(index) => self.aas_stream_songs.get(index).map(SongRef::AasStream),
            SongId::AasPcm(index) => self.aas_pcm_songs.get(index).map(SongRef::AasPcm),
            SongId::Gb(index) => self.gb_songs.get(index).map(SongRef::Gb),
            SongId::Nes(index) => self.nes_songs.get(index).map(SongRef::Nes),
            SongId::NesNative(index) => self.nes_native_songs.get(index).map(SongRef::NesNative),
            SongId::GbNative(index) => self.gb_native_songs.get(index).map(SongRef::GbNative),
            SongId::Huge(index) => self.huge_songs.get(index).map(SongRef::Huge),
            SongId::GbMusyx(index) => self.gb_musyx_songs.get(index).map(SongRef::GbMusyx),
            SongId::GbTose(index) => self.gb_tose_songs.get(index).map(SongRef::GbTose),
            SongId::GbQuickThunder(index) => self
                .gb_quickthunder_songs
                .get(index)
                .map(SongRef::GbQuickThunder),
            SongId::GbGhx(index) => self.gb_ghx_songs.get(index).map(SongRef::GbGhx),
            SongId::GbSoundSystem(index) => self
                .gb_sound_system_songs
                .get(index)
                .map(SongRef::GbSoundSystem),
            SongId::GbCarillon(index) => self.gb_carillon_songs.get(index).map(SongRef::GbCarillon),
            SongId::WsTose(index) => self.ws_tose_songs.get(index).map(SongRef::WsTose),
            SongId::NesTose(index) => self.nes_tose_songs.get(index).map(SongRef::NesTose),
            SongId::SegaPsg(index) => self.sega_psg_songs.get(index).map(SongRef::SegaPsg),
            SongId::Natsume(index) => self.natsume_songs.get(index).map(SongRef::Natsume),
            SongId::Vgm(index) => self.vgm_logs.get(index).map(SongRef::Vgm),
            SongId::Rip(index) => self.music_rips.get(index).map(SongRef::Rip),
            SongId::Module(index) => self.tracker_modules.get(index).map(SongRef::Module),
            #[cfg(not(target_arch = "wasm32"))]
            SongId::Cdda(index) => self.cdda_tracks.get(index).map(SongRef::Cdda),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn song_at_offset(&self, offset: u32) -> anyhow::Result<SongId> {
        let mut matches = self.song_ids().filter(|&id| {
            self.song(id)
                .and_then(|song| song.span())
                .is_some_and(|span| span.effective_offset == offset)
        });
        let first = matches
            .next()
            .ok_or_else(|| anyhow::anyhow!("requested song offset was not found in this scan"))?;
        anyhow::ensure!(
            matches.next().is_none(),
            "multiple songs identify this offset; select by engine and index with --audio-song-id or in Audio Explorer"
        );
        Ok(first)
    }
}

impl SongRef<'_> {
    pub const fn requires_runtime_validation(self) -> bool {
        matches!(self, Self::Huge(_))
    }

    pub fn detector_id(self) -> &'static str {
        match self {
            Self::Mp2k(_) => "mp2k-sequence",
            Self::Gax(_) => "gax3-structure",
            Self::EngineSoftware(_) => "engine-software-structure",
            Self::Krawall(_) => "krawall-driver",
            Self::GaxNative(_) => "gax-native-driver",
            Self::Musyx(_) => "musyx-driver",
            Self::Aas(_) => "aas-driver",
            Self::DescriptorMidi(_) => "gba-descriptor-midi-driver",
            Self::Nsq(_) => "gba-nsq-driver",
            Self::Radriver(_) => "gba-radriver",
            Self::Gbass(_) => "gba-gbass-driver",
            Self::AasStream(_) => "gba-aas-stream-driver",
            Self::AasPcm(_) => "gba-aas-pcm-driver",
            Self::Gb(_) => "gb-banked-driver",
            Self::Nes(_) => "nes-queue-driver",
            Self::NesNative(_) => "nes-native-driver",
            Self::GbNative(_) => "gb-native-driver",
            Self::Huge(_) => "gb-huge-driver",
            Self::GbMusyx(_) => "gb-musyx-driver",
            Self::GbTose(_) => "gb-tose-driver",
            Self::GbQuickThunder(_) => "gb-quickthunder-driver",
            Self::GbGhx(_) => "gb-ghx-driver",
            Self::GbSoundSystem(_) => "gb-sound-system-driver",
            Self::GbCarillon(_) => "gb-carillon-driver",
            Self::WsTose(_) => "ws-tose-driver",
            Self::NesTose(_) => "nes-tose-driver",
            Self::SegaPsg(_) => "sega-psg-driver",
            Self::Natsume(_) => "gba-natsume-driver",
            Self::Vgm(_) => "vgm-register-log",
            Self::Rip(rip) => rip.format.detector_id(),
            Self::Module(_) => "tracker-structure",
            #[cfg(not(target_arch = "wasm32"))]
            Self::Cdda(_) => "pce-cdda-toc",
        }
    }

    pub fn span(self) -> Option<SourceSpan> {
        match self {
            Self::Mp2k(song) => Some(song.header.into()),
            Self::Gax(song) => Some(song.header.into()),
            Self::EngineSoftware(song) => Some(song.header.into()),
            Self::Krawall(song) => Some(song.header.into()),
            Self::GaxNative(song) => Some(song.header.into()),
            Self::Musyx(song) => Some(song.header.into()),
            Self::Aas(song) => Some(song.header.into()),
            Self::DescriptorMidi(song) => Some(song.header.into()),
            Self::Nsq(song) => Some(song.header.into()),
            Self::Radriver(song) => Some(song.header.into()),
            Self::Gbass(song) => Some(song.header.into()),
            Self::AasStream(song) => Some(song.header.into()),
            Self::AasPcm(song) => Some(song.header.into()),
            Self::Gb(song) => Some(song.header.into()),
            Self::Nes(song) => Some(song.table_entry.into()),
            Self::NesNative(song) => Some(song.table_entry.into()),
            Self::GbNative(song) => Some(song.table_entry.into()),
            Self::Huge(song) => Some(SourceSpan {
                effective_offset: song.bound.song.descriptor.offset,
                byte_len: song.bound.song.descriptor.byte_len,
                canonical_cpu_address: None,
            }),
            Self::GbTose(song) => Some(SourceSpan {
                effective_offset: song.table_entry.effective_offset,
                byte_len: song.table_entry.byte_len,
                canonical_cpu_address: None,
            }),
            Self::GbQuickThunder(song) => Some(SourceSpan {
                effective_offset: song.table_entry.effective_offset,
                byte_len: song.table_entry.byte_len,
                canonical_cpu_address: None,
            }),
            Self::GbGhx(song) => Some(SourceSpan {
                effective_offset: song.table_entry.effective_offset,
                byte_len: song.table_entry.byte_len,
                canonical_cpu_address: None,
            }),
            Self::GbSoundSystem(song) => Some(SourceSpan {
                effective_offset: song.table_entry.effective_offset,
                byte_len: song.table_entry.byte_len,
                canonical_cpu_address: None,
            }),
            Self::GbCarillon(song) => Some(SourceSpan {
                effective_offset: song.table_entry.effective_offset,
                byte_len: song.table_entry.byte_len,
                canonical_cpu_address: None,
            }),
            Self::WsTose(song) => Some(SourceSpan {
                effective_offset: song.table_entry.effective_offset,
                byte_len: song.table_entry.byte_len,
                canonical_cpu_address: None,
            }),
            Self::NesTose(song) => Some(SourceSpan {
                effective_offset: song.table_entry.effective_offset,
                byte_len: song.table_entry.byte_len,
                canonical_cpu_address: None,
            }),
            Self::GbMusyx(song) => Some(SourceSpan {
                effective_offset: song.table_entry.effective_offset,
                byte_len: song.table_entry.byte_len,
                canonical_cpu_address: None,
            }),
            Self::SegaPsg(song) => Some(song.table_entry.into()),
            Self::Natsume(song) => Some(song.header.into()),
            Self::Vgm(log) => Some(log.source.into()),
            Self::Rip(rip) => Some(rip.source.into()),
            Self::Module(song) => Some(song.span.into()),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Cdda(_) => None,
        }
    }

    pub fn title(self) -> String {
        match self {
            Self::Mp2k(song) => {
                if song.table_entries.is_empty() {
                    format!("MP2k +{:06X}", song.header.effective_offset)
                } else {
                    format!(
                        "Song {}",
                        song.table_entries
                            .iter()
                            .map(|entry| entry.index.to_string())
                            .collect::<Vec<_>>()
                            .join(" / ")
                    )
                }
            }
            Self::Gax(song) => song.title.clone(),
            Self::EngineSoftware(song) => song.title.clone(),
            Self::Krawall(song) => song.title.clone(),
            Self::GaxNative(song) => song.title.clone(),
            Self::Musyx(song) => song.title.clone(),
            Self::Aas(song) => song.title.clone(),
            Self::DescriptorMidi(song) => song.title.clone(),
            Self::Nsq(song) => song.title.clone(),
            Self::Radriver(song) => song.title.clone(),
            Self::Gbass(song) => song.title.clone(),
            Self::AasStream(song) => song.title.clone(),
            Self::AasPcm(song) => song.title.clone(),
            Self::Gb(song) => format!("Song {} · {}", song.index, song.title),
            Self::Nes(song) => format!("Song {} · {}", song.index, song.title),
            Self::NesNative(song) => song.title.clone(),
            Self::GbNative(song) => song.title.clone(),
            Self::Huge(song) => format!("hUGE +{:04X}", song.bound.song.descriptor.offset),
            Self::GbMusyx(song) => song.title.clone(),
            Self::GbTose(song) => song.title.clone(),
            Self::GbQuickThunder(song) => song.title.clone(),
            Self::GbGhx(song) => song.title.clone(),
            Self::GbSoundSystem(song) => song.title.clone(),
            Self::GbCarillon(song) => song.title.clone(),
            Self::WsTose(song) => song.title.clone(),
            Self::NesTose(song) => song.title.clone(),
            Self::SegaPsg(song) => song.title.clone(),
            Self::Natsume(song) => match song.kind {
                super::natsume::NatsumeSongKind::Music => format!("Song {}", song.index),
                _ => format!("Song {} · {}", song.index, song.title),
            },
            Self::Vgm(log) => {
                if log.title.is_empty() {
                    "Untitled VGM log".to_owned()
                } else {
                    log.title.clone()
                }
            }
            Self::Rip(rip) => format!(
                "{} · {} songs",
                if rip.title.is_empty() {
                    rip.format.label()
                } else {
                    &rip.title
                },
                rip.song_count
            ),
            Self::Module(song) => {
                if song.name.is_empty() {
                    "Untitled module".to_owned()
                } else {
                    song.name.clone()
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::Cdda(song) => format!("CD track {:02}", song.number),
        }
    }

    pub fn engine(self) -> &'static str {
        match self {
            Self::Mp2k(song) => match song.engine {
                super::EngineProfile::Mp2k => "MP2k",
                super::EngineProfile::Mp2kSongId => "MP2k song-ID driver",
            },
            Self::Gax(_) => "GAX 3",
            Self::EngineSoftware(_) => "Engine Software format",
            Self::Krawall(_) => "Krawall",
            Self::GaxNative(_) => "GAX native driver",
            Self::Musyx(_) => "MusyX",
            Self::Aas(_) => "Apex Audio System",
            Self::DescriptorMidi(_) => "GBA descriptor MIDI",
            Self::Nsq(_) => "GBA NSQ/NPF",
            Self::Radriver(_) => "RADriver",
            Self::Gbass(_) => "GBASS",
            Self::AasStream(_) => "AAS stream",
            Self::AasPcm(_) => "AAS PCM",
            Self::Gb(_) => "GB banked driver",
            Self::Nes(_) => "NES queue driver",
            Self::NesNative(_) => "NES native driver",
            Self::GbNative(_) => "Game Boy native driver",
            Self::Huge(_) => "hUGEDriver (runtime validation required)",
            Self::GbMusyx(_) => "Game Boy MusyX",
            Self::GbTose(_) => "Game Boy TOSE",
            Self::GbQuickThunder(_) => "Game Boy QuickThunder",
            Self::GbGhx(_) => "Game Boy GHX",
            Self::GbSoundSystem(_) => "Game Boy Sound System",
            Self::GbCarillon(_) => "Game Boy Carillon",
            Self::WsTose(_) => "WonderSwan TOSE-style",
            Self::NesTose(_) => "NES TOSE",
            Self::SegaPsg(_) => "Sega PSG driver",
            Self::Natsume(_) => "GBA Natsume driver",
            Self::Vgm(_) => "VGM register log",
            Self::Rip(rip) => rip.format.label(),
            Self::Module(song) => song.format.label(),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Cdda(_) => "CD audio",
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn supports(self, format: super::formats::SongFormat) -> bool {
        use super::formats::SongFormat;
        if format.is_gsf()
            && matches!(
                self,
                Self::Krawall(_)
                    | Self::GaxNative(_)
                    | Self::Musyx(_)
                    | Self::Aas(_)
                    | Self::DescriptorMidi(_)
                    | Self::Nsq(_)
                    | Self::Radriver(_)
                    | Self::Gbass(_)
                    | Self::AasStream(_)
                    | Self::AasPcm(_)
            )
        {
            return true;
        }
        if let Some(native) = super::native_rips::supported_format(self)
            && matches!(
                (native, format),
                (super::native_rips::NativeRipFormat::Gbs, SongFormat::Gbs)
                    | (super::native_rips::NativeRipFormat::Nsf, SongFormat::Nsf)
                    | (super::native_rips::NativeRipFormat::Nsf, SongFormat::Nsfe)
                    | (super::native_rips::NativeRipFormat::Sgc, SongFormat::Sgc)
            )
        {
            return true;
        }
        match self {
            Self::Mp2k(_) => matches!(
                format,
                SongFormat::Midi
                    | SongFormat::SoundFont
                    | SongFormat::Dls
                    | SongFormat::Sfz
                    | SongFormat::MidiSoundFont
                    | SongFormat::Gsf
                    | SongFormat::MiniGsfPack
                    | SongFormat::Audio(_)
                    | SongFormat::MappedAssets
            ),
            Self::Gax(song) => {
                matches!(format, SongFormat::MappedAssets)
                    || (song.xm_exportable
                        && matches!(
                            format,
                            SongFormat::Xm | SongFormat::TrackerPack | SongFormat::Audio(_)
                        ))
            }
            Self::EngineSoftware(_) => matches!(
                format,
                SongFormat::MappedAssets
                    | SongFormat::Xm
                    | SongFormat::TrackerPack
                    | SongFormat::Audio(_)
            ),
            Self::Krawall(_) | Self::GaxNative(_) | Self::Musyx(_) | Self::Aas(_) => {
                matches!(format, SongFormat::MappedAssets | SongFormat::Audio(_))
            }
            Self::Nsq(_)
            | Self::Radriver(_)
            | Self::Gbass(_)
            | Self::AasStream(_)
            | Self::AasPcm(_)
            | Self::SegaPsg(_)
            | Self::NesNative(_)
            | Self::GbNative(_)
            | Self::GbMusyx(_)
            | Self::GbTose(_)
            | Self::GbQuickThunder(_)
            | Self::GbGhx(_)
            | Self::GbSoundSystem(_)
            | Self::GbCarillon(_)
            | Self::WsTose(_)
            | Self::NesTose(_) => {
                matches!(format, SongFormat::MappedAssets | SongFormat::Audio(_))
            }
            Self::Huge(_) => matches!(format, SongFormat::Audio(_) | SongFormat::Gbs),
            Self::DescriptorMidi(_) => matches!(
                format,
                SongFormat::MappedAssets | SongFormat::Audio(_) | SongFormat::Midi
            ),
            Self::Gb(song) => {
                format == SongFormat::MappedAssets
                    || (format == SongFormat::Midi && song.midi_exportable)
                    || (matches!(format, SongFormat::Audio(_))
                        && super::gb_music::native::supports_native(song))
            }
            Self::Module(song) => {
                matches!(format, SongFormat::MappedAssets | SongFormat::TrackerPack)
                    || matches!(
                        (song.format, format),
                        (super::tracker::EmbeddedFormat::Xm, SongFormat::Xm)
                            | (super::tracker::EmbeddedFormat::Mod, SongFormat::Mod)
                            | (super::tracker::EmbeddedFormat::S3m, SongFormat::S3m)
                            | (super::tracker::EmbeddedFormat::It, SongFormat::It)
                    )
            }
            Self::Nes(song) => {
                format == SongFormat::MappedAssets
                    || (format == SongFormat::Midi && song.midi_exportable)
                    || (matches!(format, SongFormat::Audio(_))
                        && super::nes_music::native::supports_native(song))
            }
            Self::Natsume(song) => {
                format == SongFormat::MappedAssets
                    || (song.kind == super::natsume::NatsumeSongKind::Music
                        && (matches!(format, SongFormat::Audio(_)) || format.is_gsf()))
            }
            Self::Vgm(log) => {
                matches!(format, SongFormat::Vgm | SongFormat::MappedAssets)
                    || (format == SongFormat::Vgz && log.encoding == super::vgm::VgmEncoding::Gzip)
                    || (log.sn_playback.is_some() && matches!(format, SongFormat::Audio(_)))
            }
            Self::Rip(rip) => {
                format == SongFormat::MappedAssets
                    || matches!(
                        (rip.format, format),
                        (super::rips::RipFormat::Gbs, SongFormat::Gbs)
                            | (super::rips::RipFormat::Nsf, SongFormat::Nsf)
                            | (super::rips::RipFormat::Nsfe, SongFormat::Nsfe)
                    )
            }
            Self::Cdda(_) => matches!(format, SongFormat::Audio(_)),
        }
    }
}
