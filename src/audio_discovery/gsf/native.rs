use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use crate::audio_discovery::{
    RomSpan,
    catalog::{SongId, SongRef},
    extract::ExtractionRequest,
    formats::SongFormat,
    media::{ScanInput, ScanManifest},
    pcm::song::PcmSong,
    render::RenderOptions,
};

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;

enum NativeSong {
    Pcm(PcmSong),
    Natsume(Box<crate::audio_discovery::natsume::NatsumeSong>),
}

pub(crate) struct NativeGsfRequest {
    bytes: Arc<[u8]>,
    sha256: String,
    song: NativeSong,
    options: RenderOptions,
    metadata: Value,
    title: String,
    game: String,
    mini_filename: String,
    pack: bool,
}

impl NativeGsfRequest {
    pub(crate) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        id: SongId,
        format: SongFormat,
        options: RenderOptions,
    ) -> Result<Self> {
        let song = manifest.scan.song(id).context("select a native GBA song")?;
        ensure!(
            format.is_gsf() && song.supports(format) && !matches!(song, SongRef::Mp2k(_)),
            "selection has no native GSF exporter"
        );
        ensure!(
            input.system == Some(zeff_emu_common::system::System::Gba),
            "GSF requires GBA media"
        );
        super::timing::validate_options(options)?;
        ensure!(
            u16::from(options.fade_seconds) <= options.max_seconds,
            "GSF fade exceeds the requested duration"
        );
        ExtractionRequest::prepare(
            input,
            manifest,
            song.span().context("song has no source range")?,
            "Native GSF",
        )?;
        let owned = match song {
            SongRef::Natsume(song) => NativeSong::Natsume(Box::new(song.clone())),
            _ => NativeSong::Pcm(PcmSong::from_ref(song).context("song has no native GBA player")?),
        };
        let filename =
            crate::audio_discovery::naming::song(input, &manifest.scan, id, SongFormat::Gsf);
        Ok(Self {
            bytes: Arc::clone(&input.bytes),
            sha256: manifest
                .scan
                .media
                .sha256
                .clone()
                .context("GSF source has no identity")?,
            song: owned,
            options,
            metadata: json!({
                "classification": manifest.classification(song),
                "schema": "zeff-native-gsf-export/1", "source": manifest.source,
                "transforms": manifest.transforms, "media": manifest.scan.media,
                "analysis_profile": manifest.analysis_profile, "selection": id,
                "song_detector": song.detector_id(),
                "limitations": [
                    "Runs the qualified original driver and selected sound with fresh state; retains cartridge data and appends a playback bootstrap.",
                    "Length and fade are requested player tags. Automatic loop detection and preview startup sample alignment are not claimed."
                ]
            }),
            title: super::tag_text(filename.strip_suffix(".gsf").unwrap_or(&filename)),
            game: super::tag_text(input.display_name.as_deref().unwrap_or("Unknown game")),
            mini_filename: format!("{}.minigsf", filename.strip_suffix(".gsf").unwrap()),
            pack: format == SongFormat::MiniGsfPack,
        })
    }

    pub(crate) fn write_new(
        mut self,
        path: &Path,
        cancel: &AtomicBool,
        progress: &AtomicU32,
    ) -> Result<()> {
        ensure!(!cancel.load(Ordering::Relaxed), "GSF export cancelled");
        ensure!(
            zeff_firmware::sha256_hex(&self.bytes) == self.sha256,
            "GSF source identity changed"
        );
        let rom = match &self.song {
            NativeSong::Pcm(song) => prepare_pcm(&self.bytes, song, cancel)?,
            NativeSong::Natsume(song) => {
                crate::audio_discovery::natsume::preview::prepare_rom(&self.bytes, song, cancel)?
            }
        };
        self.metadata["native"] = match &self.song {
            NativeSong::Pcm(song) => serde_json::to_value(song)?,
            NativeSong::Natsume(song) => serde_json::to_value(song)?,
        };
        progress.store(30, Ordering::Relaxed);
        let fade = u16::from(self.options.fade_seconds);
        let length = self.options.max_seconds - fade;
        self.metadata["entry_address"] = json!(0x0800_0000u32);
        self.metadata["prepared_sha256"] = json!(zeff_firmware::sha256_hex(&rom));
        self.metadata["playback"] = json!({"length_seconds": length, "fade_seconds": fade});
        let mut tags = vec![
            ("utf8", "1".to_owned()),
            ("game", self.game),
            ("title", self.title),
            ("ripper", "zeff-boy".to_owned()),
            ("length", length.to_string()),
            ("fade", fade.to_string()),
        ];
        let output = if self.pack {
            super::overlay::pack(
                &self.bytes,
                &rom,
                &self.mini_filename,
                &tags,
                self.metadata,
                cancel,
            )?
        } else {
            tags.push(("comment", super::metadata_text(&self.metadata)?));
            super::codec::encode(0x0800_0000, 0x0800_0000, &rom, &tags, cancel)?
        };
        crate::audio_discovery::assets::publish_bytes(path, &output, cancel, progress)
    }
}

pub(super) fn prepare_pcm(bytes: &[u8], song: &PcmSong, cancel: &AtomicBool) -> Result<Vec<u8>> {
    use zeff_audio_discovery as audio;
    macro_rules! ready {
        ($module:ident, $song:expr) => {{
            let prepared = audio::$module::prepare_rom(bytes, $song, cancel)?;
            autonomous(prepared.bytes, prepared.wait_loop)
        }};
    }
    match song {
        PcmSong::Krawall(song) => audio::krawall::prepare_rom(bytes, song, cancel),
        PcmSong::GaxNative(song) => audio::gax_native::prepare_rom(bytes, song, cancel),
        PcmSong::Musyx(song) => ready!(musyx, song),
        PcmSong::Aas(song) => ready!(aas, song),
        PcmSong::DescriptorMidi(song) => ready!(descriptor_midi, song),
        PcmSong::Nsq(song) => ready!(nsq, song),
        PcmSong::Radriver(song) => ready!(radriver, song),
        PcmSong::Gbass(song) => ready!(gbass, song),
        PcmSong::AasStream(song) => ready!(aas_stream, song),
        PcmSong::AasPcm(song) => ready!(aas_pcm, song),
        _ => anyhow::bail!("song has no native GBA executable"),
    }
}

fn autonomous(mut bytes: Vec<u8>, wait: RomSpan) -> Result<Vec<u8>> {
    ensure!(
        wait.byte_len == 12 && wait.effective_offset.is_multiple_of(4),
        "unexpected native ready loop"
    );
    let start = wait.effective_offset as usize;
    let code = bytes
        .get_mut(start..start + 12)
        .context("native ready loop is outside cartridge")?;
    let load = u32::from_le_bytes(code[..4].try_into()?);
    ensure!(
        [0xE590_1000, 0xE590_1004].contains(&load)
            && code[4..8] == 0xE351_0001u32.to_le_bytes()
            && code[8..] == 0x1AFF_FFFCu32.to_le_bytes(),
        "native ready-loop contract changed"
    );
    // A standalone player acknowledges startup itself, retaining the driver's ACK state.
    for (slot, word) in code.as_chunks_mut::<4>().0.iter_mut().zip([
        0xE3A0_1001,
        0xE580_1000 | (load & 4),
        0xE351_0001,
    ]) {
        slot.copy_from_slice(&word.to_le_bytes());
    }
    Ok(bytes)
}
