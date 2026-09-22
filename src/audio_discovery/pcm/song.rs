use std::sync::atomic::AtomicBool;

use anyhow::Result;
use serde::Serialize;

use super::{PcmSession, tracker::TrackerSession};
use crate::audio_discovery::{RomSpan, catalog::SongRef, render::RenderOptions};

mod export;
pub(crate) use export::PcmExportRequest;

#[derive(Clone, Serialize)]
#[serde(tag = "engine", content = "song", rename_all = "snake_case")]
pub(crate) enum PcmSong {
    Huge(Box<zeff_audio_discovery::huge::catalog::HugeSong>),
    Vgm(Box<zeff_audio_discovery::vgm::VgmLog>),
    EngineSoftware(Box<zeff_audio_discovery::engine_software::EngineSoftwareSong>),
    Gax(Box<zeff_audio_discovery::gax::GaxSong>),
    Krawall(Box<zeff_audio_discovery::krawall::KrawallSong>),
    GaxNative(Box<zeff_audio_discovery::gax_native::GaxNativeSong>),
    Musyx(Box<zeff_audio_discovery::musyx::MusyxSong>),
    Aas(Box<zeff_audio_discovery::aas::AasSong>),
    DescriptorMidi(Box<zeff_audio_discovery::descriptor_midi::DescriptorMidiSong>),
    Nsq(Box<zeff_audio_discovery::nsq::NsqSong>),
    Radriver(Box<zeff_audio_discovery::radriver::RadriverSong>),
    Gbass(Box<zeff_audio_discovery::gbass::GbassSong>),
    AasStream(Box<zeff_audio_discovery::aas_stream::AasStreamSong>),
    AasPcm(Box<zeff_audio_discovery::aas_pcm::AasPcmSong>),
    NesNative(Box<zeff_audio_discovery::nes_native::NesNativeSong>),
    NesQueue(Box<zeff_audio_discovery::nes_music::NesSong>),
    GbNative(Box<zeff_audio_discovery::gb_native::GbNativeSong>),
    GbMusyx(Box<zeff_audio_discovery::gb_musyx::GbMusyxSong>),
    GbTose(Box<zeff_audio_discovery::gb_tose::GbToseSong>),
    #[serde(rename = "gb_quickthunder")]
    GbQuickThunder(Box<zeff_audio_discovery::gb_quickthunder::GbQuickThunderSong>),
    GbGhx(Box<zeff_audio_discovery::gb_ghx::GbGhxSong>),
    GbSoundSystem(Box<zeff_audio_discovery::gb_sound_system::GbSoundSystemSong>),
    GbCarillon(Box<zeff_audio_discovery::gb_carillon::GbCarillonSong>),
    WsTose(Box<zeff_audio_discovery::ws_tose::WsToseSong>),
    NesTose(Box<zeff_audio_discovery::nes_tose::NesToseSong>),
    GbBanked(Box<zeff_audio_discovery::gb_music::GbSong>),
    SegaPsg(Box<zeff_audio_discovery::sega_psg::SegaPsgSong>),
}

impl PcmSong {
    pub(crate) fn can_play(song: SongRef<'_>) -> bool {
        matches!(
            song,
            SongRef::Huge(_)
                | SongRef::EngineSoftware(_)
                | SongRef::Krawall(_)
                | SongRef::GaxNative(_)
                | SongRef::Musyx(_)
                | SongRef::Aas(_)
                | SongRef::DescriptorMidi(_)
                | SongRef::Nsq(_)
                | SongRef::Radriver(_)
                | SongRef::Gbass(_)
                | SongRef::AasStream(_)
                | SongRef::AasPcm(_)
                | SongRef::NesNative(_)
                | SongRef::GbNative(_)
                | SongRef::GbMusyx(_)
                | SongRef::GbTose(_)
                | SongRef::GbQuickThunder(_)
                | SongRef::GbGhx(_)
                | SongRef::GbSoundSystem(_)
                | SongRef::GbCarillon(_)
                | SongRef::WsTose(_)
                | SongRef::NesTose(_)
                | SongRef::SegaPsg(_)
        ) || matches!(song, SongRef::Vgm(log) if log.sn_playback.is_some())
            || matches!(song, SongRef::Gax(song) if song.xm_exportable)
            || matches!(song, SongRef::Nes(song) if zeff_audio_discovery::nes_music::native::supports_native(song))
            || matches!(song, SongRef::Gb(song) if zeff_audio_discovery::gb_music::native::supports_native(song))
    }
    pub(crate) fn is_native(song: SongRef<'_>) -> bool {
        matches!(
            song,
            SongRef::Huge(_)
                | SongRef::Krawall(_)
                | SongRef::GaxNative(_)
                | SongRef::Musyx(_)
                | SongRef::Aas(_)
                | SongRef::DescriptorMidi(_)
                | SongRef::Nsq(_)
                | SongRef::Radriver(_)
                | SongRef::Gbass(_)
                | SongRef::AasStream(_)
                | SongRef::AasPcm(_)
                | SongRef::NesNative(_)
                | SongRef::GbNative(_)
                | SongRef::GbMusyx(_)
                | SongRef::GbTose(_)
                | SongRef::GbQuickThunder(_)
                | SongRef::GbGhx(_)
                | SongRef::GbSoundSystem(_)
                | SongRef::GbCarillon(_)
                | SongRef::WsTose(_)
                | SongRef::NesTose(_)
                | SongRef::SegaPsg(_)
        ) || matches!(song, SongRef::Nes(song) if zeff_audio_discovery::nes_music::native::supports_native(song))
            || matches!(song, SongRef::Gb(song) if zeff_audio_discovery::gb_music::native::supports_native(song))
    }
    pub(crate) fn from_ref(song: SongRef<'_>) -> Option<Self> {
        match song {
            SongRef::Huge(song) => Some(Self::Huge(Box::new(song.clone()))),
            SongRef::Vgm(log) if log.sn_playback.is_some() => {
                Some(Self::Vgm(Box::new(log.clone())))
            }
            SongRef::EngineSoftware(song) => Some(Self::EngineSoftware(Box::new(song.clone()))),
            SongRef::Gax(song) if song.xm_exportable => Some(Self::Gax(Box::new(song.clone()))),
            SongRef::Krawall(song) => Some(Self::Krawall(Box::new(song.clone()))),
            SongRef::GaxNative(song) => Some(Self::GaxNative(Box::new(song.clone()))),
            SongRef::Musyx(song) => Some(Self::Musyx(Box::new(song.clone()))),
            SongRef::Aas(song) => Some(Self::Aas(Box::new(song.clone()))),
            SongRef::DescriptorMidi(song) => Some(Self::DescriptorMidi(Box::new(song.clone()))),
            SongRef::Nsq(song) => Some(Self::Nsq(Box::new(song.clone()))),
            SongRef::Radriver(song) => Some(Self::Radriver(Box::new(song.clone()))),
            SongRef::Gbass(song) => Some(Self::Gbass(Box::new(song.clone()))),
            SongRef::AasStream(song) => Some(Self::AasStream(Box::new(song.clone()))),
            SongRef::AasPcm(song) => Some(Self::AasPcm(Box::new(song.clone()))),
            SongRef::NesNative(song) => Some(Self::NesNative(Box::new(song.clone()))),
            SongRef::Nes(song)
                if zeff_audio_discovery::nes_music::native::supports_native(song) =>
            {
                Some(Self::NesQueue(Box::new(song.clone())))
            }
            SongRef::GbNative(song) => Some(Self::GbNative(Box::new(song.clone()))),
            SongRef::GbMusyx(song) => Some(Self::GbMusyx(Box::new(song.clone()))),
            SongRef::GbTose(song) => Some(Self::GbTose(Box::new(song.clone()))),
            SongRef::GbQuickThunder(song) => Some(Self::GbQuickThunder(Box::new(song.clone()))),
            SongRef::GbGhx(song) => Some(Self::GbGhx(Box::new(song.clone()))),
            SongRef::GbSoundSystem(song) => Some(Self::GbSoundSystem(Box::new(song.clone()))),
            SongRef::GbCarillon(song) => Some(Self::GbCarillon(Box::new(song.clone()))),
            SongRef::WsTose(song) => Some(Self::WsTose(Box::new(song.clone()))),
            SongRef::NesTose(song) => Some(Self::NesTose(Box::new(song.clone()))),
            SongRef::Gb(song) if zeff_audio_discovery::gb_music::native::supports_native(song) => {
                Some(Self::GbBanked(Box::new(song.clone())))
            }
            SongRef::SegaPsg(song) => Some(Self::SegaPsg(Box::new(song.clone()))),
            _ => None,
        }
    }

    pub(crate) fn session(
        &self,
        bytes: &[u8],
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Box<dyn PcmSession>> {
        super::validate_options(options)?;
        super::check_cancel(cancel)?;
        let (xm, warnings) = match self {
            Self::Huge(song) => {
                return Ok(Box::new(super::huge::HugeSession::new(
                    bytes, song, options, cancel,
                )?));
            }
            Self::Vgm(log) => {
                return Ok(Box::new(super::vgm::VgmSession::new(
                    bytes, log, options, cancel,
                )?));
            }
            Self::GbTose(song) => {
                let prepared = zeff_audio_discovery::gb_tose::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gb_banked::GbBankedSession::new_tose(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::GbGhx(song) => {
                let prepared = zeff_audio_discovery::gb_ghx::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gb_banked::GbBankedSession::new_ghx(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::GbSoundSystem(song) => {
                let prepared =
                    zeff_audio_discovery::gb_sound_system::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(
                    super::gb_banked::GbBankedSession::new_sound_system(
                        prepared,
                        options,
                        song.warnings.clone(),
                        cancel,
                    )?,
                ));
            }
            Self::GbCarillon(song) => {
                let prepared = zeff_audio_discovery::gb_carillon::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gb_banked::GbBankedSession::new_carillon(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::GbQuickThunder(song) => {
                let prepared =
                    zeff_audio_discovery::gb_quickthunder::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(
                    super::gb_banked::GbBankedSession::new_quickthunder(
                        prepared,
                        options,
                        song.warnings.clone(),
                        cancel,
                    )?,
                ));
            }
            Self::WsTose(song) => {
                let prepared = zeff_audio_discovery::ws_tose::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::ws::WsSession::new(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::NesTose(song) => {
                let prepared = zeff_audio_discovery::nes_tose::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::nes::NesSession::new(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::GbMusyx(song) => {
                let prepared = zeff_audio_discovery::gb_musyx::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gb_banked::GbBankedSession::new_musyx(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::NesQueue(song) => {
                let prepared =
                    zeff_audio_discovery::nes_music::native::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::nes::NesSession::new(
                    prepared, options,
                    vec!["Starts the selected queue with cleared sound state; gameplay transitions and a prior area song are not reproduced.".into()],
                    cancel,
                )?));
            }
            Self::GbBanked(song) => {
                let prepared =
                    zeff_audio_discovery::gb_music::native::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gb_banked::GbBankedSession::new(
                    prepared,
                    options,
                    Vec::new(),
                    cancel,
                )?));
            }
            Self::GbNative(song) => {
                let prepared = zeff_audio_discovery::gb_native::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gb::GbSession::new(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::NesNative(song) => {
                let prepared = zeff_audio_discovery::nes_native::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::nes::NesSession::new(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::SegaPsg(song) => {
                let prepared = zeff_audio_discovery::sega_psg::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::sega::SegaSession::new(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Nsq(song) => {
                let prepared = zeff_audio_discovery::nsq::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::DescriptorMidi(song) => {
                let prepared =
                    zeff_audio_discovery::descriptor_midi::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Aas(song) => {
                let prepared = zeff_audio_discovery::aas::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::AasPcm(song) => {
                let prepared = zeff_audio_discovery::aas_pcm::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::AasStream(song) => {
                let prepared = zeff_audio_discovery::aas_stream::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Gbass(song) => {
                let prepared = zeff_audio_discovery::gbass::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Radriver(song) => {
                let prepared = zeff_audio_discovery::radriver::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Musyx(song) => {
                let prepared = zeff_audio_discovery::musyx::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Krawall(song) => {
                let rom = zeff_audio_discovery::krawall::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new(
                    rom,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::GaxNative(song) => {
                let rom = zeff_audio_discovery::gax_native::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new(
                    rom,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::EngineSoftware(song) => (
                zeff_audio_discovery::engine_software::to_xm(bytes, song, cancel)?,
                song.warnings.clone(),
            ),
            Self::Gax(song) => {
                let module = zeff_audio_discovery::gax::project(bytes, song, cancel)?;
                (
                    zeff_audio_discovery::tracker::xm::encode(&module, cancel)?,
                    song.warnings
                        .iter()
                        .map(|w| format!("+{:06X}: {}", w.offset, w.reason))
                        .collect(),
                )
            }
        };
        Ok(Box::new(TrackerSession::from_xm(
            &xm, options, warnings, cancel,
        )?))
    }

    fn mapped_spans(&self) -> Option<&[RomSpan]> {
        Some(match self {
            Self::Huge(_) | Self::GbBanked(_) | Self::NesQueue(_) | Self::Vgm(_) => return None,
            Self::EngineSoftware(song) => &song.mapped_spans,
            Self::Gax(song) => &song.mapped_spans,
            Self::Krawall(song) => &song.mapped_spans,
            Self::GaxNative(song) => &song.mapped_spans,
            Self::Musyx(song) => &song.mapped_spans,
            Self::Aas(song) => &song.mapped_spans,
            Self::DescriptorMidi(song) => &song.mapped_spans,
            Self::Nsq(song) => &song.mapped_spans,
            Self::Radriver(song) => &song.mapped_spans,
            Self::Gbass(song) => &song.mapped_spans,
            Self::AasStream(song) => &song.mapped_spans,
            Self::AasPcm(song) => &song.mapped_spans,
            Self::NesNative(song) => &song.mapped_spans,
            Self::GbNative(song) => &song.mapped_spans,
            Self::GbMusyx(song) => &song.mapped_spans,
            Self::GbTose(song) => &song.mapped_spans,
            Self::GbQuickThunder(song) => &song.mapped_spans,
            Self::GbGhx(song) => &song.mapped_spans,
            Self::GbSoundSystem(song) => &song.mapped_spans,
            Self::GbCarillon(song) => &song.mapped_spans,
            Self::WsTose(song) => &song.mapped_spans,
            Self::NesTose(song) => &song.mapped_spans,
            Self::SegaPsg(song) => &song.mapped_spans,
        })
    }
}
