use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use super::catalog::SongId;
use super::extract::ExtractionRequest;
use super::formats::SongFormat;
use super::media::{ScanInput, ScanManifest};
use super::render::RenderOptions;
use super::{SongCandidate, naming};

pub(crate) const MAX_NATIVE_PATCHES: usize = 64;

pub(crate) mod bootstrap;
mod codec;
mod driver;
pub(crate) mod native;
mod overlay;
#[cfg(test)]
mod tests;
mod timing;

pub(crate) struct GsfExportRequest {
    bytes: Arc<[u8]>,
    sha256: String,
    driver: driver::Driver,
    song: SongCandidate,
    options: RenderOptions,
    metadata: Value,
    game: String,
    mini_filename: String,
    pack: bool,
}

pub(crate) fn available(input: &ScanInput, manifest: &ScanManifest, index: usize) -> bool {
    if input.system != Some(zeff_emu_common::system::System::Gba) {
        return false;
    }
    let Some(song) = manifest.scan.candidates.get(index) else {
        return false;
    };
    driver::inspect(
        &input.bytes,
        song,
        &manifest.scan.song_tables,
        &AtomicBool::new(false),
    )
    .is_ok()
}

impl GsfExportRequest {
    pub(crate) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        index: usize,
        format: SongFormat,
        options: RenderOptions,
    ) -> Result<Self> {
        ensure!(format.is_gsf(), "selected format is not GSF");
        ensure!(
            input.system == Some(zeff_emu_common::system::System::Gba),
            "GSF export requires GBA media"
        );
        let song = manifest
            .scan
            .candidates
            .get(index)
            .context("select an MP2k song before exporting GSF")?;
        ExtractionRequest::prepare(input, manifest, song.header, "GSF song")?;
        let driver = driver::inspect(
            &input.bytes,
            song,
            &manifest.scan.song_tables,
            &AtomicBool::new(false),
        )?;
        timing::validate_options(options)?;
        let sha256 = manifest
            .scan
            .media
            .sha256
            .clone()
            .context("GSF source has no identity")?;
        let filename = naming::song(input, &manifest.scan, SongId::Mp2k(index), SongFormat::Gsf);
        let mini_filename = format!("{}.minigsf", filename.strip_suffix(".gsf").unwrap());
        Ok(Self {
            bytes: Arc::clone(&input.bytes),
            sha256,
            driver,
            song: song.clone(),
            options,
            metadata: json!({
                "schema": "zeff-gsf-export/1",
                "analysis_profile": manifest.analysis_profile,
                "display_name": manifest.display_name,
                "source": manifest.source,
                "transforms": manifest.transforms,
                "media": manifest.scan.media,
                "detector": manifest.scan.detector,
                "detector_version": manifest.scan.detector_version,
                "scan_status": manifest.scan.status,
                "song": song,
                "limitations": [
                    "Runs a verified stock MP2k driver with fresh player state. Custom engine hooks and game-managed streaming are not supported.",
                    "The original cartridge is retained except for the reset branch and an appended playback bootstrap. This is not an optimized GSF rip.",
                    "Playback length is estimated from decoded sequence timing; the GSF player applies length and fade tags. Game execution and exact hardware capture are not reproduced."
                ],
            }),
            game: tag_text(input.display_name.as_deref().unwrap_or("Unknown game")),
            mini_filename,
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
            "loaded media does not match the GSF source identity"
        );
        let mut bootstrap = driver::build(&self.bytes, &self.driver, cancel)?;
        progress.store(30, Ordering::Relaxed);
        let playback = timing::playback(&self.song, &self.bytes, self.options, cancel)?;
        self.metadata["driver"] = bootstrap.metadata;
        self.metadata["entry_address"] = json!(bootstrap.entry_address);
        self.metadata["song_number_offset"] = json!(bootstrap.song_number_offset);
        self.metadata["song_number"] = json!(bootstrap.song_number);
        self.metadata["playback"] = serde_json::to_value(&playback)?;
        let mut tags = vec![
            ("utf8", "1".to_owned()),
            ("game", self.game.clone()),
            ("title", format!("Song {}", bootstrap.song_number)),
            ("ripper", "zeff-boy".to_owned()),
            ("length", playback.length_tag()),
            ("fade", playback.fade_tag()),
        ];
        let data = if self.pack {
            let offset = bootstrap.song_number_offset as usize;
            bootstrap
                .patched_rom
                .get_mut(offset..offset + 4)
                .context("GSF song overlay is outside the cartridge")?
                .fill(0);
            let library = codec::encode(
                bootstrap.entry_address,
                0x0800_0000,
                &bootstrap.patched_rom,
                &[
                    ("utf8", "1".to_owned()),
                    ("game", self.game),
                    ("ripper", "zeff-boy".to_owned()),
                ],
                cancel,
            )?;
            let library_hash = zeff_firmware::sha256_hex(&library);
            let library_name = format!("audio-{}.gsflib", &library_hash[..16]);
            self.metadata["library"] = json!({"path": library_name, "sha256": library_hash});
            self.metadata["minigsf"] = json!(self.mini_filename);
            tags.push(("_lib", library_name.clone()));
            tags.push(("comment", metadata_text(&self.metadata)?));
            let mini = codec::encode(
                bootstrap.entry_address,
                0x0800_0000 + bootstrap.song_number_offset,
                &bootstrap.song_number.to_le_bytes(),
                &tags,
                cancel,
            )?;
            let mut bundle = super::bundle::Bundle::new();
            bundle.add(&library_name, &library)?;
            bundle.add(&self.mini_filename, &mini)?;
            bundle.add("manifest.json", metadata_text(&self.metadata)?.as_bytes())?;
            bundle.finish()?
        } else {
            tags.push(("comment", metadata_text(&self.metadata)?));
            codec::encode(
                bootstrap.entry_address,
                0x0800_0000,
                &bootstrap.patched_rom,
                &tags,
                cancel,
            )?
        };
        progress.store(90, Ordering::Relaxed);
        super::assets::publish_bytes(path, &data, cancel, progress)
    }
}

fn tag_text(text: &str) -> String {
    text.chars()
        .take(256)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn metadata_text(metadata: &Value) -> Result<String> {
    let text = serde_json::to_string(metadata)?;
    ensure!(
        text.len() <= 1024 * 1024 - 4096,
        "GSF metadata exceeds its size limit"
    );
    Ok(text)
}
