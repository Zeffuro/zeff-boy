use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::catalog::SongId;
use super::export::SongExportRequest;
use super::formats::{FormatAvailability, SongFormat};
use super::media::{ScanInput, ScanManifest};
use super::render::RenderOptions;

mod mini;
#[cfg(test)]
mod tests;

pub(crate) struct BatchExportRequest {
    input: Arc<ScanInput>,
    manifest: ScanManifest,
    format: SongFormat,
    entries: Vec<PlannedSong>,
}

struct PlannedSong {
    id: SongId,
    engine: String,
    ordinal: usize,
    title: String,
    options: Option<RenderOptions>,
}

#[derive(Default, Serialize)]
pub(crate) struct BatchSummary {
    pub(crate) exported: usize,
    pub(crate) skipped: usize,
    pub(crate) failed: usize,
}

impl BatchSummary {
    pub(crate) fn message(&self) -> String {
        format!(
            "Exported {} entries · {} skipped · {} failed. See batch-report.json in the ZIP.",
            self.exported, self.skipped, self.failed
        )
    }
}

#[derive(Serialize)]
struct SongResult {
    song: SongId,
    classification: zeff_audio_discovery::classification::AudioClassification,
    title: String,
    status: &'static str,
    options: Option<RenderOptions>,
    files: Vec<OutputFile>,
    reason: Option<String>,
}

#[derive(Clone, Serialize)]
struct OutputFile {
    path: String,
    bytes: u64,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_entry: Option<String>,
}

impl BatchExportRequest {
    pub(crate) fn prepare(
        input: &Arc<ScanInput>,
        manifest: &ScanManifest,
        format: SongFormat,
        mut options_for: impl FnMut(SongId) -> Result<RenderOptions>,
    ) -> Result<Self> {
        ensure!(
            format.available(),
            "this export format is unavailable in this build"
        );
        let entries = manifest
            .scan
            .song_ids()
            .map(|id| {
                let song = manifest.scan.song(id).expect("catalog entry");
                let identity = serde_json::to_value(id)?;
                Ok(PlannedSong {
                    id,
                    engine: identity["engine"]
                        .as_str()
                        .context("song engine is missing")?
                        .to_owned(),
                    ordinal: identity["index"]
                        .as_u64()
                        .context("song index is missing")? as usize,
                    title: song.title(),
                    options: if song.supports(format) {
                        Some(options_for(id)?)
                    } else {
                        None
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            entries.iter().any(|entry| entry.options.is_some()),
            "no entries support this export format"
        );
        Ok(Self {
            input: Arc::clone(input),
            manifest: manifest.clone(),
            format,
            entries,
        })
    }

    pub(crate) fn write_new(
        self,
        path: &Path,
        cancel: &AtomicBool,
        progress: &AtomicU32,
    ) -> Result<BatchSummary> {
        check_cancel(cancel)?;
        ensure!(!path.try_exists()?, "batch output already exists");
        if self.input.cdda.is_none() {
            ensure!(
                self.manifest.scan.media.sha256.as_deref()
                    == Some(zeff_firmware::sha256_hex(&self.input.bytes).as_str()),
                "batch source identity changed"
            );
        }
        let scratch = tempfile::tempdir().context("could not create batch workspace")?;
        let mut summary = BatchSummary::default();
        crate::platform::write_new_file_atomically_streamed(
            path,
            |file| {
                let mut archive = Archive::new(file);
                let mut results = Vec::with_capacity(self.entries.len());
                for (index, entry) in self.entries.iter().enumerate() {
                    check_cancel(cancel)?;
                    let mut result = SongResult {
                        song: entry.id,
                        classification: self.manifest.classification(
                            self.manifest
                                .scan
                                .song(entry.id)
                                .expect("planned catalog entry"),
                        ),
                        title: entry.title.clone(),
                        status: "skipped",
                        options: entry.options,
                        files: Vec::new(),
                        reason: None,
                    };
                    let unsupported = entry.options.is_none()
                        || (self.format.is_gsf()
                            && matches!(entry.id, SongId::Mp2k(i) if !super::gsf::available(&self.input, &self.manifest, i)));
                    if unsupported {
                        summary.skipped += 1;
                        result.reason = Some(format!(
                            "{} is unavailable for this entry",
                            self.format.info().label
                        ));
                    } else {
                        let temporary = scratch
                            .path()
                            .join(format!("{index}.{}", self.format.info().extension));
                        let exported = SongExportRequest::prepare(
                            &self.input,
                            &self.manifest,
                            entry.id,
                            self.format,
                            entry.options.unwrap(),
                        )
                        .and_then(|request| {
                            request.write_new(&temporary, cancel, &AtomicU32::new(0))
                        })
                        .and_then(|()| {
                            if self.format == SongFormat::MiniGsfPack {
                                mini::PreparedPack::read(&archive, &temporary, entry, cancel)
                                    .map(Some)
                            } else {
                                Ok(None)
                            }
                        });
                        check_cancel(cancel)?;
                        match exported {
                            Ok(pack) => {
                                result.files = if let Some(pack) = pack {
                                    pack.append(&mut archive, cancel)?
                                } else {
                                    let name = super::naming::song(
                                        &self.input,
                                        &self.manifest.scan,
                                        entry.id,
                                        self.format,
                                    );
                                    let name =
                                        format!("{}/{:04} - {name}", entry.engine, entry.ordinal);
                                    vec![archive.add_file(&name, &temporary, cancel)?]
                                };
                                result.status = "exported";
                                summary.exported += 1;
                            }
                            Err(error) => {
                                result.status = "failed";
                                result.reason = Some(format!("{error:#}"));
                                summary.failed += 1;
                            }
                        }
                        if temporary.try_exists()? {
                            std::fs::remove_file(temporary)?;
                        }
                    }
                    results.push(result);
                    progress.store(
                        ((index + 1) * 95 / self.entries.len()) as u32,
                        Ordering::Relaxed,
                    );
                }
                let catalog_path = scratch.path().join("scan-report.json");
                let mut catalog = std::io::BufWriter::new(CancellableWriter {
                    file: File::create(&catalog_path)?,
                    cancel,
                });
                serde_json::to_writer_pretty(&mut catalog, &self.manifest)?;
                catalog.flush()?;
                drop(catalog);
                let scan_report = archive.add_file("scan-report.json", &catalog_path, cancel)?;
                let report = serde_json::to_vec_pretty(&serde_json::json!({
                    "schema": "zeff-audio-batch-export/1", "format": self.format.info().id,
                    "source": self.manifest.source, "transforms": self.manifest.transforms,
                    "media": self.manifest.scan.media, "disc": self.manifest.disc,
                    "analysis_profile": self.manifest.analysis_profile,
                    "detector_version": self.manifest.scan.detector_version,
                    "scan_status": self.manifest.scan.status, "scan_report": scan_report,
                    "summary": summary, "entries": results,
                }))?;
                archive.add_bytes("batch-report.json", &report, cancel)?;
                archive.finish()?;
                Ok(())
            },
            || check_cancel(cancel),
        )?;
        progress.store(100, Ordering::Relaxed);
        Ok(summary)
    }
}

struct Archive<'a> {
    writer: zip::ZipWriter<&'a mut File>,
    files: BTreeMap<String, OutputFile>,
}

impl<'a> Archive<'a> {
    fn new(file: &'a mut File) -> Self {
        Self {
            writer: zip::ZipWriter::new(file),
            files: BTreeMap::new(),
        }
    }

    fn add_file(&mut self, name: &str, path: &Path, cancel: &AtomicBool) -> Result<OutputFile> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        self.add_reader(name, &mut file, length, cancel)
    }

    fn add_bytes(&mut self, name: &str, bytes: &[u8], cancel: &AtomicBool) -> Result<OutputFile> {
        self.add_reader(
            name,
            &mut std::io::Cursor::new(bytes),
            bytes.len() as u64,
            cancel,
        )
    }

    fn add_reader(
        &mut self,
        name: &str,
        input: &mut (impl Read + Seek),
        length: u64,
        cancel: &AtomicBool,
    ) -> Result<OutputFile> {
        validate_name(name)?;
        check_cancel(cancel)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        let mut bytes = 0;
        loop {
            check_cancel(cancel)?;
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            ensure!(bytes <= length, "batch entry grew while reading");
            hash.update(&buffer[..count]);
        }
        ensure!(bytes == length, "batch entry changed while reading");
        let hash = const_hex::encode(hash.finalize());
        self.check_existing(name, bytes, &hash)?;
        if let Some(previous) = self.files.get(name) {
            return Ok(previous.clone());
        }
        input.rewind()?;
        self.writer.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .large_file(length >= u64::from(u32::MAX)),
        )?;
        let mut written = 0;
        let mut written_hash = Sha256::new();
        loop {
            check_cancel(cancel)?;
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            self.writer.write_all(&buffer[..count])?;
            written_hash.update(&buffer[..count]);
            written += count as u64;
        }
        ensure!(
            written == bytes && const_hex::encode(written_hash.finalize()) == hash,
            "batch entry changed while writing"
        );
        let output = OutputFile {
            path: name.to_owned(),
            bytes,
            sha256: hash,
            original_entry: None,
        };
        self.files.insert(name.to_owned(), output.clone());
        Ok(output)
    }

    fn check_existing(&self, name: &str, bytes: u64, hash: &str) -> Result<()> {
        validate_name(name)?;
        if let Some(previous) = self.files.get(name) {
            ensure!(
                previous.bytes == bytes && previous.sha256 == hash,
                "conflicting batch library name"
            );
        }
        Ok(())
    }

    fn finish(self) -> Result<()> {
        self.writer.finish()?;
        Ok(())
    }
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "batch export cancelled");
    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && !name.starts_with('/')
            && !name.contains(['\\', ':'])
            && name
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."),
        "invalid batch entry path"
    );
    Ok(())
}

struct CancellableWriter<'a> {
    file: File,
    cancel: &'a AtomicBool,
}

impl Write for CancellableWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("batch export cancelled"));
        }
        self.file.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
