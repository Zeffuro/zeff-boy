use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, ensure};
use serde::Serialize;

#[cfg(test)]
use super::RomSpan;
use super::media::{ScanInput, ScanManifest, SourceIdentity};
use super::{ScanStatus, SourceSpan};

const MAX_SELECTION_BYTES: usize = 32 * 1024 * 1024;
const MAX_LABEL_BYTES: usize = 4 * 1024;
const MAX_METADATA_BYTES: usize = 1024 * 1024;
const SELECTION_ENTRY: &str = "selection.json";
const DATA_ENTRY: &str = "data.bin";

pub(crate) struct ExtractionRequest {
    bytes: Arc<[u8]>,
    start: usize,
    end: usize,
    metadata: ExtractionMetadata,
}

#[derive(Clone, Serialize)]
struct ExtractionMetadata {
    schema: &'static str,
    kind: &'static str,
    limitations: [&'static str; 3],
    label: String,
    span: SourceSpan,
    source: Option<SourceIdentity>,
    transforms: Option<Vec<crate::mods::ModApplicationStep>>,
    scan: ScanIdentity,
}

#[derive(Clone, Serialize)]
struct ScanIdentity {
    manifest_schema: &'static str,
    scan_schema: &'static str,
    analysis_profile: &'static str,
    detector: &'static str,
    detector_version: u32,
    applicable_detectors: &'static [super::detectors::DetectorDescriptor],
    detector_outcomes: Vec<super::DetectorOutcome>,
    status: ScanStatus,
    media_byte_len: u64,
    media_sha256: String,
}

#[derive(Serialize)]
struct SelectionManifest<'a> {
    #[serde(flatten)]
    metadata: &'a ExtractionMetadata,
    raw_byte_len: usize,
    raw_sha256: &'a str,
}

impl ExtractionRequest {
    pub(crate) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        span: impl Into<SourceSpan>,
        label: &str,
    ) -> anyhow::Result<Self> {
        let span = span.into();
        ensure!(
            manifest.scan.media.system == input.media_system_id()
                && input.bytes.len() <= super::MAX_ROM_BYTES,
            "raw selection requires bounded media and a matching scan system"
        );
        ensure!(
            manifest.source.as_ref()
                == input
                    .provenance
                    .as_ref()
                    .map(|provenance| &provenance.source)
                && manifest.transforms.as_ref()
                    == input
                        .provenance
                        .as_ref()
                        .map(|provenance| &provenance.transforms),
            "scan provenance does not match the loaded media"
        );
        ensure!(
            !label.trim().is_empty(),
            "raw selection label must not be empty"
        );
        ensure!(
            label.len() <= MAX_LABEL_BYTES,
            "raw selection label exceeds the {MAX_LABEL_BYTES}-byte limit"
        );

        let start = usize::try_from(span.effective_offset)
            .context("raw selection offset does not fit the current platform")?;
        let byte_len = usize::try_from(span.byte_len)
            .context("raw selection length does not fit the current platform")?;
        ensure!(byte_len > 0, "raw selection must contain at least one byte");
        ensure!(
            byte_len <= MAX_SELECTION_BYTES,
            "raw selection exceeds the {MAX_SELECTION_BYTES}-byte limit"
        );
        let end = start
            .checked_add(byte_len)
            .context("raw selection range overflows")?;
        ensure!(
            end <= input.bytes.len(),
            "raw selection range is outside the loaded media"
        );
        let expected_address = 0x0800_0000_u32
            .checked_add(span.effective_offset)
            .context("raw selection CPU address overflows")?;
        if let Some(address) = span.canonical_cpu_address {
            ensure!(
                (input.system == Some(zeff_emu_common::system::System::Gba)
                    && address == expected_address)
                    || super::sega_psg::source_span_matches(&manifest.scan.media, span)
                    || super::nes_native::source_span_matches(&manifest.scan.media, span)
                    || super::gb_native::source_span_matches(&manifest.scan.media, span),
                "raw selection CPU address does not map to its effective offset"
            );
        }

        let input_len = u64::try_from(input.bytes.len()).context("loaded media is too large")?;
        ensure!(
            manifest.analysis_profile == input.analysis_profile,
            "scan manifest analysis profile does not match the loaded media"
        );
        ensure!(
            manifest.scan.media.byte_len == input_len,
            "scan manifest media length does not match the loaded media"
        );
        let media_sha256 = manifest
            .scan
            .media
            .sha256
            .as_deref()
            .context("scan manifest has no media SHA-256 identity")?;
        ensure!(
            is_sha256_hex(media_sha256),
            "scan manifest media SHA-256 identity is invalid"
        );

        let metadata = ExtractionMetadata {
            schema: "zeff-audio-raw-selection/1",
            kind: "raw_rom_span",
            limitations: [
                "This archive contains one raw effective-ROM range, not a complete song or asset collection.",
                "The data is not decoded audio and no engine or song-table identity is asserted.",
                "The selection is meaningful only with the recorded scan identity and load-time provenance.",
            ],
            label: label.to_owned(),
            span,
            source: manifest.source.clone(),
            transforms: manifest.transforms.clone(),
            scan: ScanIdentity {
                manifest_schema: manifest.schema,
                scan_schema: manifest.scan.schema,
                analysis_profile: manifest.analysis_profile,
                detector: manifest.scan.detector,
                detector_version: manifest.scan.detector_version,
                applicable_detectors: manifest.scan.applicable_detectors,
                detector_outcomes: manifest.scan.detector_outcomes.clone(),
                status: manifest.scan.status,
                media_byte_len: input_len,
                media_sha256: media_sha256.to_owned(),
            },
        };
        let placeholder_hash = "0".repeat(64);
        let metadata_bytes = serde_json::to_vec(&SelectionManifest {
            metadata: &metadata,
            raw_byte_len: byte_len,
            raw_sha256: &placeholder_hash,
        })?;
        ensure!(
            metadata_bytes.len() <= MAX_METADATA_BYTES,
            "raw selection metadata exceeds the {MAX_METADATA_BYTES}-byte limit"
        );

        Ok(Self {
            bytes: Arc::clone(&input.bytes),
            start,
            end,
            metadata,
        })
    }

    #[cfg(test)]
    pub(crate) fn write_new(self, path: &Path) -> anyhow::Result<()> {
        self.write_new_cancellable(path, &std::sync::atomic::AtomicBool::new(false))
    }

    pub(crate) fn write_new_cancellable(
        self,
        path: &Path,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> anyhow::Result<()> {
        let check_cancelled = || {
            ensure!(
                !cancel.load(std::sync::atomic::Ordering::Relaxed),
                "export cancelled"
            );
            Ok(())
        };
        check_cancelled()?;
        let current_sha256 = zeff_firmware::sha256_hex(&self.bytes);
        ensure!(
            current_sha256 == self.metadata.scan.media_sha256,
            "loaded media no longer matches the scan SHA-256 identity"
        );

        let raw = &self.bytes[self.start..self.end];
        let raw_sha256 = zeff_firmware::sha256_hex(raw);
        let selection_json = serde_json::to_vec_pretty(&SelectionManifest {
            metadata: &self.metadata,
            raw_byte_len: raw.len(),
            raw_sha256: &raw_sha256,
        })?;
        ensure!(
            selection_json.len() <= MAX_METADATA_BYTES,
            "raw selection metadata exceeds the {MAX_METADATA_BYTES}-byte limit"
        );
        let archive = encode_archive(&selection_json, raw)?;

        crate::platform::write_new_file_atomically_validated_cancellable(
            path,
            &archive,
            |file| validate_archive(file, &selection_json, raw),
            check_cancelled,
        )
        .with_context(|| format!("failed to create raw audio selection {}", path.display()))
    }
}

fn encode_archive(selection_json: &[u8], raw: &[u8]) -> anyhow::Result<Vec<u8>> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default());
    writer.start_file(SELECTION_ENTRY, options)?;
    writer.write_all(selection_json)?;
    writer.start_file(DATA_ENTRY, options)?;
    writer.write_all(raw)?;
    Ok(writer.finish()?.into_inner())
}

fn validate_archive(
    file: &mut std::fs::File,
    selection_json: &[u8],
    raw: &[u8],
) -> anyhow::Result<()> {
    file.rewind()?;
    let mut archive = zip::ZipArchive::new(file).context("temporary selection ZIP is invalid")?;
    ensure!(
        archive.len() == 2,
        "temporary selection ZIP must contain exactly two entries"
    );

    let mut selection = archive.by_index(0)?;
    ensure!(
        selection.name() == SELECTION_ENTRY,
        "temporary selection ZIP has an unexpected metadata entry"
    );
    ensure!(
        selection.size() == selection_json.len() as u64,
        "temporary selection ZIP metadata length is wrong"
    );
    let mut actual_selection = Vec::with_capacity(selection_json.len());
    selection.read_to_end(&mut actual_selection)?;
    ensure!(
        actual_selection == selection_json,
        "temporary selection ZIP metadata changed before publication"
    );
    drop(selection);

    let mut data = archive.by_index(1)?;
    ensure!(
        data.name() == DATA_ENTRY,
        "temporary selection ZIP has an unexpected data entry"
    );
    ensure!(
        data.size() == raw.len() as u64,
        "temporary selection ZIP data length is wrong"
    );
    let mut offset = 0_usize;
    let mut buffer = [0; 8192];
    loop {
        let read = data.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let end = offset
            .checked_add(read)
            .context("temporary selection ZIP data length overflows")?;
        ensure!(
            end <= raw.len() && buffer[..read] == raw[offset..end],
            "temporary selection ZIP data changed before publication"
        );
        offset = end;
    }
    ensure!(
        offset == raw.len(),
        "temporary selection ZIP data was truncated"
    );
    Ok(())
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::audio_discovery::ScanLimits;
    use crate::audio_discovery::media::{ScanProvenance, SelectedMemberIdentity};
    use zeff_emu_common::system::System;

    fn input_and_manifest() -> (ScanInput, ScanManifest, Vec<u8>) {
        let bytes = crate::audio_discovery::test_support::gba_fixture();
        let input = ScanInput {
            #[cfg(not(target_arch = "wasm32"))]
            cdda: None,
            system: Some(System::Gba),
            standalone_audio: None,
            bytes: bytes.clone().into(),
            provenance: Some(Arc::new(ScanProvenance {
                source: SourceIdentity {
                    kind: "synthetic_loaded_gba",
                    sha256: zeff_firmware::sha256_hex(&bytes),
                    len: bytes.len(),
                    container: None,
                    selected_member: Some(SelectedMemberIdentity {
                        name: "fixture.gba".to_owned(),
                        sha256: zeff_firmware::sha256_hex(&bytes),
                        len: bytes.len(),
                    }),
                },
                transforms: Vec::new(),
            })),
            analysis_profile: "test-loaded-effective-v1",
            display_name: None,
        };
        let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        (input, manifest, bytes)
    }

    fn span(offset: usize, len: usize) -> RomSpan {
        RomSpan {
            effective_offset: offset as u32,
            byte_len: len as u32,
            canonical_cpu_address: 0x0800_0000 + offset as u32,
        }
    }

    #[test]
    fn exports_exact_bytes_provenance_and_a_deterministic_archive() -> anyhow::Result<()> {
        let directory = crate::test_support::test_directory("audio-raw-selection")?;
        let (input, manifest, bytes) = input_and_manifest();
        let selected = span(0x110, 32);
        let first = directory.path().join("first.zip");
        let second = directory.path().join("second.zip");

        ExtractionRequest::prepare(&input, &manifest, selected, "fixture track")?
            .write_new(&first)?;
        ExtractionRequest::prepare(&input, &manifest, selected, "fixture track")?
            .write_new(&second)?;

        let first_bytes = std::fs::read(&first)?;
        assert_eq!(first_bytes, std::fs::read(&second)?);
        let mut archive = zip::ZipArchive::new(Cursor::new(first_bytes))?;
        assert_eq!(archive.len(), 2);
        assert_eq!(archive.by_index(0)?.name(), SELECTION_ENTRY);
        assert_eq!(archive.by_index(1)?.name(), DATA_ENTRY);
        let selection: serde_json::Value = serde_json::from_reader(archive.by_index(0)?)?;
        let expected = &bytes[0x110..0x130];
        assert_eq!(selection["schema"], "zeff-audio-raw-selection/1");
        assert_eq!(selection["kind"], "raw_rom_span");
        assert_eq!(selection["label"], "fixture track");
        assert_eq!(selection["span"]["effective_offset"], 0x110);
        assert_eq!(selection["raw_byte_len"], expected.len());
        assert_eq!(selection["raw_sha256"], zeff_firmware::sha256_hex(expected));
        assert_eq!(selection["source"]["kind"], "synthetic_loaded_gba");
        assert_eq!(
            selection["source"]["selected_member"]["name"],
            "fixture.gba"
        );
        assert_eq!(selection["transforms"], serde_json::json!([]));
        assert_eq!(
            selection["scan"]["media_sha256"],
            zeff_firmware::sha256_hex(&bytes)
        );
        let mut data = Vec::new();
        archive.by_index(1)?.read_to_end(&mut data)?;
        assert_eq!(data, expected);
        Ok(())
    }

    #[test]
    fn stale_media_is_rejected_before_creating_output() -> anyhow::Result<()> {
        let directory = crate::test_support::test_directory("audio-raw-selection-stale")?;
        let (input, manifest, mut bytes) = input_and_manifest();
        bytes[0x110] ^= 0xFF;
        let stale_input = ScanInput {
            #[cfg(not(target_arch = "wasm32"))]
            cdda: None,
            system: input.system,
            standalone_audio: input.standalone_audio,
            bytes: bytes.into(),
            provenance: input.provenance.clone(),
            analysis_profile: input.analysis_profile,
            display_name: input.display_name.clone(),
        };
        let output = directory.path().join("stale.zip");

        let request =
            ExtractionRequest::prepare(&stale_input, &manifest, span(0x110, 16), "stale")?;
        assert!(request.write_new(&output).is_err());
        assert!(!output.exists());
        Ok(())
    }

    #[test]
    fn invalid_or_overflowing_ranges_are_rejected() {
        let (input, manifest, bytes) = input_and_manifest();
        assert!(ExtractionRequest::prepare(&input, &manifest, span(0, 0), "empty").is_err());
        assert!(
            ExtractionRequest::prepare(&input, &manifest, span(bytes.len(), 1), "past").is_err()
        );
        assert!(
            ExtractionRequest::prepare(
                &input,
                &manifest,
                RomSpan {
                    effective_offset: 0,
                    byte_len: 1,
                    canonical_cpu_address: 0x0800_0001,
                },
                "mismapped",
            )
            .is_err()
        );
        assert!(
            ExtractionRequest::prepare(
                &input,
                &manifest,
                RomSpan {
                    effective_offset: u32::MAX,
                    byte_len: 1,
                    canonical_cpu_address: u32::MAX,
                },
                "overflow",
            )
            .is_err()
        );
        assert!(
            ExtractionRequest::prepare(
                &input,
                &manifest,
                span(0, MAX_SELECTION_BYTES + 1),
                "too large",
            )
            .is_err()
        );
        assert!(
            ExtractionRequest::prepare(
                &input,
                &manifest,
                span(0, 1),
                &"x".repeat(MAX_LABEL_BYTES + 1),
            )
            .is_err()
        );
    }

    #[test]
    fn equal_bytes_with_different_provenance_or_profile_are_rejected() {
        let (mut input, manifest, _) = input_and_manifest();
        input.analysis_profile = "other-profile";
        assert!(ExtractionRequest::prepare(&input, &manifest, span(0, 1), "test").is_err());
        input.analysis_profile = manifest.analysis_profile;
        Arc::make_mut(input.provenance.as_mut().unwrap())
            .source
            .kind = "different-source";
        assert!(ExtractionRequest::prepare(&input, &manifest, span(0, 1), "test").is_err());
        input.provenance = None;
        assert!(ExtractionRequest::prepare(&input, &manifest, span(0, 1), "test").is_err());
    }

    #[test]
    fn existing_output_is_not_overwritten() -> anyhow::Result<()> {
        let directory = crate::test_support::test_directory("audio-raw-selection-no-clobber")?;
        let (input, manifest, _) = input_and_manifest();
        let output = directory.path().join("selection.zip");
        std::fs::write(&output, b"keep")?;

        assert!(
            ExtractionRequest::prepare(&input, &manifest, span(0x110, 16), "fixture")?
                .write_new(&output)
                .is_err()
        );
        assert_eq!(std::fs::read(output)?, b"keep");
        Ok(())
    }
}
