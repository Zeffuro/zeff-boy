use std::collections::BTreeSet;
use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use anyhow::{Context, ensure};
use serde_json::{Value, json};

#[cfg(test)]
use super::audio_file::wav;
use super::audio_file::{self, AudioData, AudioInfo};
use super::extract::ExtractionRequest;
pub(crate) use super::formats::ExportKind;
use super::formats::SongFormat;
use super::media::{ScanInput, ScanManifest};
use super::{
    RomSpan, SampleInventory, SongCandidate, ToneInventory, midi, projection, render, sf2,
};

const MAX_ARTIFACT_BYTES: usize = 128 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 1024 * 1024;

pub(crate) struct AudioExportRequest {
    bytes: Arc<[u8]>,
    song: SongCandidate,
    metadata: Value,
    sha256: String,
    kind: ExportKind,
    mixer_rate: Option<u32>,
}

impl AudioExportRequest {
    pub(crate) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        index: usize,
        kind: ExportKind,
    ) -> anyhow::Result<Self> {
        let song = manifest
            .scan
            .candidates
            .get(index)
            .context("select a song before exporting")?;
        if let ExportKind::Song { format, .. } = kind {
            ensure!(
                super::catalog::SongRef::Mp2k(song).supports(format),
                "selected MP2k song does not support this export format"
            );
        }
        if let ExportKind::Song {
            format: SongFormat::Audio(_),
            options,
        } = kind
        {
            render::validate_sample_rate(options.sample_rate)?;
        }
        ExtractionRequest::prepare(input, manifest, song.header, "Song assets")?;
        let sha256 = manifest
            .scan
            .media
            .sha256
            .clone()
            .context("scan has no ROM identity")?;
        let (mixer_rate, mixer_evidence) = infer_mixer_rate(&input.bytes, manifest, song);
        let metadata = json!({
            "schema": "zeff-audio-export/1",
            "analysis_profile": manifest.analysis_profile,
            "display_name": manifest.display_name,
            "detector": manifest.scan.detector,
            "detector_version": manifest.scan.detector_version,
            "applicable_detectors": manifest.scan.applicable_detectors,
            "detector_outcomes": manifest.scan.detector_outcomes,
            "song_detector": "mp2k-sequence",
            "scan_status": manifest.scan.status,
            "media": manifest.scan.media,
            "source": manifest.source,
            "transforms": manifest.transforms,
            "song": song,
            "inferred_mixer_rate_hz": mixer_rate,
            "mixer_rate_evidence": mixer_evidence,
            "scope": "Selected song and its mapped referenced instruments/keys; not the entire voicegroup",
        });
        ensure!(
            metadata_bytes(&metadata)?.len() <= MAX_METADATA_BYTES,
            "export metadata exceeds its size limit"
        );
        Ok(Self {
            bytes: Arc::clone(&input.bytes),
            song: song.clone(),
            metadata,
            sha256,
            kind,
            mixer_rate,
        })
    }

    pub(crate) fn write_new(
        mut self,
        path: &Path,
        cancel: &AtomicBool,
        progress: &AtomicU32,
    ) -> anyhow::Result<()> {
        check_cancel(cancel)?;
        ensure!(
            zeff_firmware::sha256_hex(&self.bytes) == self.sha256,
            "loaded ROM does not match the scan SHA-256 identity"
        );
        let data = match self.kind {
            ExportKind::Song {
                format: SongFormat::Midi,
                options,
            } => midi_bytes(
                &self.song,
                &self.bytes,
                options,
                self.metadata,
                cancel,
                progress,
            )?,
            ExportKind::Song {
                format: SongFormat::MappedAssets,
                ..
            } => {
                self.metadata["kind"] = json!("mapped_original_song_assets");
                self.metadata["limitations"] = json!([
                    "Only validated mapped ROM ranges are included. Runtime synthesis and player state are not executed.",
                    "The recorded scan warnings describe unresolved portions. This is an asset archive, not standalone playback."
                ]);
                mapped_assets(&self.bytes, &self.song, &mut self.metadata, cancel)?
            }
            ExportKind::Sample {
                format,
                sample: selected,
            } => {
                let sample = samples(&self.song)
                    .find(|sample| **sample == selected)
                    .context("select sample data in this song before exporting decoded audio")?;
                let decoded = projection::pcm_sample_with_cancel(&self.bytes, sample, cancel)?;
                self.metadata["kind"] = json!("decoded_original_pcm");
                self.metadata["sample"] = json!(sample);
                self.metadata["limitations"] = json!([
                    "PCM/BDPCM is decoded to signed 16-bit points with the verified playback direction. Reverse samples ignore source loop flags. Playback rate is rounded to integer Hz; original frequency and source loop boundaries are retained above.",
                    "This sample contains no instrument envelope, track effects, or engine mixing. Vorbis additionally applies lossy compression."
                ]);
                audio_file::encode(
                    format,
                    AudioData {
                        pcm: &decoded.pcm,
                        channels: 1,
                        sample_rate: decoded.integer_rate(),
                        loop_range: decoded.loop_range,
                        pitch: Some((decoded.original_pitch, decoded.pitch_correction())),
                    },
                    &metadata_bytes(&self.metadata)?,
                    cancel,
                )?
            }
            ExportKind::Song { format, options } => {
                self.metadata["kind"] = json!("instrument_projection");
                self.metadata["limitations"] = json!([
                    "Approximate instrument envelopes, PSG timbres and mixing. Not an emulation or a bit-exact recording of the game.",
                    "Only referenced instrument keys are mapped. Program numbers preserve MP2k voice indices; bank 128 duplicates bank 0 for percussion-aware renderers.",
                    "Only the SoundFont adapter extends short samples and loops or applies equivalent octave rate/root mappings to satisfy SF2 limits. Other formats use the common unpadded sample bank.",
                    "Fixed PCM uses the uniquely inferred static mixer rate; runtime mixer changes are not observed. PSG noise uses key-specific clock projections and does not follow track pitch changes.",
                    "Camelot pulse width is frozen at its first mixer frame. Pseudo-saw filter-rate dependence is approximated. Triangle uses a 64-point cycle."
                ]);
                let bank = projection::instrument_bank_with_cancel(
                    &self.bytes,
                    &self.song,
                    ascii_json(&self.metadata)?,
                    self.mixer_rate,
                    cancel,
                )?;
                check_cancel(cancel)?;
                match format {
                    SongFormat::SoundFont => validated_soundfont(&bank)?,
                    SongFormat::Dls => super::dls::encode(&bank)?,
                    SongFormat::Sfz => super::sfz::encode(&bank, self.metadata, cancel)?,
                    SongFormat::MidiSoundFont => {
                        let soundfont = validated_soundfont(&bank)?;
                        let midi = midi_bytes(
                            &self.song,
                            &self.bytes,
                            options,
                            self.metadata.clone(),
                            cancel,
                            progress,
                        )?;
                        self.metadata["kind"] = json!("midi_soundfont_pack");
                        self.metadata["sequence_options"] = json!(options);
                        let mut bundle = super::bundle::Bundle::new();
                        bundle.add("song.mid", &midi)?;
                        bundle.add("song.sf2", &soundfont)?;
                        bundle.add("manifest.json", &metadata_bytes(&self.metadata)?)?;
                        bundle.finish()?
                    }
                    SongFormat::Audio(format) => {
                        let soundfont = validated_soundfont(&bank)?;
                        drop(bank);
                        let mut spool = tempfile::tempfile()
                            .context("could not create temporary PCM storage")?;
                        let summary = {
                            let mut writer = std::io::BufWriter::new(&mut spool);
                            let summary = render::render_into(
                                &self.song,
                                &self.bytes,
                                &soundfont,
                                options,
                                cancel,
                                progress,
                                &mut |block| {
                                    for sample in block {
                                        writer.write_all(&sample.to_le_bytes())?;
                                    }
                                    Ok(())
                                },
                            )?;
                            writer.flush()?;
                            summary
                        };
                        self.metadata["kind"] = json!("approximate_song_render");
                        self.metadata["render_options"] = json!(options);
                        self.metadata["render_warnings"] = json!(summary.warnings);
                        self.metadata["sample_rate"] = json!(summary.sample_rate);
                        self.metadata["frames"] = json!(summary.frames);
                        self.metadata["audio_format"] = json!(format.extension());
                        let metadata = metadata_bytes(&self.metadata)?;
                        let info = AudioInfo {
                            frames: summary.frames as u64,
                            channels: 2,
                            sample_rate: summary.sample_rate,
                            loop_range: None,
                            pitch: None,
                        };
                        progress.store(99, Ordering::Relaxed);
                        crate::platform::write_new_file_atomically_streamed(
                            path,
                            |file| {
                                audio_file::encode_to(
                                    format, &mut spool, info, &metadata, cancel, file,
                                )
                            },
                            || check_cancel(cancel),
                        )
                        .with_context(|| format!("failed to create {}", path.display()))?;
                        progress.store(100, Ordering::Relaxed);
                        return Ok(());
                    }
                    SongFormat::Midi | SongFormat::MappedAssets => {
                        unreachable!("handled without projecting instruments")
                    }
                    SongFormat::Xm
                    | SongFormat::Mod
                    | SongFormat::S3m
                    | SongFormat::It
                    | SongFormat::TrackerPack
                    | SongFormat::Vgm
                    | SongFormat::Gbs
                    | SongFormat::Nsf
                    | SongFormat::Sgc
                    | SongFormat::Vgz
                    | SongFormat::Gsf
                    | SongFormat::MiniGsfPack => {
                        anyhow::bail!("selected MP2k song does not support this export format")
                    }
                }
            }
        };
        publish_bytes(path, &data, cancel, progress)
    }
}

pub(super) fn publish_bytes(
    path: &Path,
    data: &[u8],
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> anyhow::Result<()> {
    ensure!(
        data.len() <= MAX_ARTIFACT_BYTES,
        "export exceeds the 128 MiB limit"
    );
    check_cancel(cancel)?;
    crate::platform::write_new_file_atomically_validated_cancellable(
        path,
        data,
        |file| {
            file.rewind()?;
            let mut buffer = [0; 64 * 1024];
            for expected in data.chunks(buffer.len()) {
                check_cancel(cancel)?;
                file.read_exact(&mut buffer[..expected.len()])?;
                ensure!(
                    &buffer[..expected.len()] == expected,
                    "export verification failed"
                );
            }
            ensure!(
                file.read(&mut buffer[..1])? == 0,
                "export contains unexpected trailing data"
            );
            Ok(())
        },
        || check_cancel(cancel),
    )
    .with_context(|| format!("failed to create {}", path.display()))?;
    progress.store(100, Ordering::Relaxed);
    Ok(())
}

fn validated_soundfont(bank: &super::bank::InstrumentBank) -> anyhow::Result<Vec<u8>> {
    let data = sf2::encode(bank)?;
    rustysynth::SoundFont::new(&mut Cursor::new(&data))
        .context("SoundFont consumer rejected the export")?;
    Ok(data)
}

fn midi_bytes(
    song: &SongCandidate,
    bytes: &[u8],
    options: super::render::RenderOptions,
    metadata: Value,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> anyhow::Result<Vec<u8>> {
    let data = midi::encode(
        song,
        bytes,
        midi::MidiOptions {
            loops: options.loops,
            playback_gain: options.playback_gain,
            max_seconds: options.max_seconds,
            skip_channel10: options.skip_channel10,
            bank_select: options.bank_select,
        },
        metadata,
        cancel,
        progress,
    )?;
    rustysynth::MidiFile::new(&mut Cursor::new(&data))
        .context("MIDI consumer rejected the export")?;
    Ok(data)
}

fn check_cancel(cancel: &AtomicBool) -> anyhow::Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "export cancelled");
    Ok(())
}

pub(super) fn infer_mixer_rate(
    bytes: &[u8],
    manifest: &ScanManifest,
    song: &SongCandidate,
) -> (Option<u32>, Vec<Value>) {
    const SAMPLES_PER_FRAME: [u32; 12] =
        [96, 132, 176, 224, 264, 304, 352, 448, 528, 608, 672, 704];
    let mut rates = BTreeSet::new();
    let mut evidence = Vec::new();
    for table in &manifest.scan.song_tables {
        if !song
            .table_entries
            .iter()
            .any(|entry| entry.table_offset == table.table.effective_offset)
        {
            continue;
        }
        let Some(mode) = super::word(
            bytes,
            table.settings_fields.sound_mode.effective_offset as usize,
        ) else {
            continue;
        };
        let index = ((mode >> 16) & 15) as usize;
        let Some(points) = index
            .checked_sub(1)
            .and_then(|index| SAMPLES_PER_FRAME.get(index))
        else {
            continue;
        };
        let rate = (597_275 * points + 5_000) / 10_000;
        rates.insert(rate);
        evidence.push(json!({"table_offset": table.table.effective_offset, "settings": table.settings, "settings_fields": table.settings_fields, "sound_mode": mode, "rate_index": index, "samples_per_frame": points, "inferred_rate_hz": rate}));
    }
    (
        if rates.len() == 1 {
            rates.first().copied()
        } else {
            None
        },
        evidence,
    )
}

pub(crate) fn samples(song: &SongCandidate) -> impl Iterator<Item = &SampleInventory> {
    tones(song).filter_map(|tone| tone.sample.as_ref())
}

fn tones(song: &SongCandidate) -> impl Iterator<Item = &ToneInventory> {
    song.instruments.iter().flat_map(|instrument| {
        std::iter::once(&instrument.tone).chain(
            instrument
                .regions
                .iter()
                .filter_map(|region| region.tone.as_ref()),
        )
    })
}

fn metadata_bytes(metadata: &Value) -> anyhow::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(metadata)?;
    ensure!(
        bytes.len() <= MAX_METADATA_BYTES,
        "export metadata exceeds the 1 MiB limit"
    );
    Ok(bytes)
}

fn ascii_json(metadata: &Value) -> anyhow::Result<String> {
    use std::fmt::Write;
    let text = String::from_utf8(metadata_bytes(metadata)?)?;
    let mut ascii = String::new();
    for ch in text.chars() {
        if ch.is_ascii() {
            ascii.push(ch);
        } else {
            for unit in ch.encode_utf16(&mut [0; 2]) {
                write!(ascii, "\\u{unit:04x}")?;
            }
        }
    }
    ensure!(
        ascii.len() <= MAX_METADATA_BYTES,
        "SoundFont metadata exceeds the 1 MiB limit"
    );
    Ok(ascii)
}

fn mapped_assets(
    bytes: &[u8],
    song: &SongCandidate,
    metadata: &mut Value,
    cancel: &AtomicBool,
) -> anyhow::Result<Vec<u8>> {
    let mut spans = BTreeSet::from([song.header]);
    for track in &song.tracks {
        spans.extend(track.spans.iter().copied());
    }
    for instrument in &song.instruments {
        spans.extend(instrument.key_map);
    }
    for tone in tones(song) {
        spans.insert(tone.descriptor);
        spans.extend(tone.sample_header);
        spans.extend(tone.waveform);
        if let Some(recipe) = &tone.synthesis {
            spans.insert(recipe.parameters);
            spans.extend(recipe.engine_evidence);
        }
        if let Some(sample) = &tone.sample {
            spans.insert(sample.data);
        }
    }
    let mut entries = Vec::new();
    let mut total_bytes = 0;
    for span in spans {
        let raw = span_bytes(bytes, span)?;
        total_bytes += raw.len();
        ensure!(
            total_bytes <= MAX_ARTIFACT_BYTES / 2,
            "mapped asset payload exceeds its 64 MiB limit"
        );
        entries.push((
            format!(
                "rom/{:08X}-{:08X}.bin",
                span.effective_offset, span.byte_len
            ),
            span,
            raw,
        ));
    }
    metadata["entries"] = json!(entries.iter().map(|(name, span, raw)| json!({"path":name,"span":span,"sha256":zeff_firmware::sha256_hex(raw)})).collect::<Vec<_>>());
    let mut archive = super::bundle::Bundle::new();
    archive.add("manifest.json", &metadata_bytes(metadata)?)?;
    for (name, _, raw) in entries {
        check_cancel(cancel)?;
        archive.add(&name, raw)?;
    }
    archive.finish()
}

fn span_bytes(bytes: &[u8], span: RomSpan) -> anyhow::Result<&[u8]> {
    let start = span.effective_offset as usize;
    let end = start
        .checked_add(span.byte_len as usize)
        .context("asset span overflows")?;
    ensure!(
        span.canonical_cpu_address == 0x0800_0000 + span.effective_offset,
        "asset address mismatch"
    );
    bytes
        .get(start..end)
        .context("asset range outside the scanned ROM")
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_emu_common::system::System;

    fn fixture() -> (ScanInput, ScanManifest) {
        let input = ScanInput {
            #[cfg(not(target_arch = "wasm32"))]
            cdda: None,
            system: Some(System::Gba),
            standalone_audio: None,
            bytes: super::super::test_support::fixture().into(),
            provenance: None,
            analysis_profile: "asset-test",
            display_name: None,
        };
        let manifest = input.analyze(Default::default(), &AtomicBool::new(false));
        (input, manifest)
    }

    fn test_path(extension: &str) -> std::path::PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "zeff-audio-export-{}-{time}-{sequence}.{extension}",
            std::process::id()
        ))
    }

    fn chunks(bytes: &[u8]) -> Vec<(&[u8], &[u8])> {
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8,
            bytes.len()
        );
        let mut position = 12;
        let mut result = Vec::new();
        while position < bytes.len() {
            let length =
                u32::from_le_bytes(bytes[position + 4..position + 8].try_into().unwrap()) as usize;
            result.push((
                &bytes[position..position + 4],
                &bytes[position + 8..position + 8 + length],
            ));
            position += 8 + length + (length & 1);
        }
        assert_eq!(position, bytes.len());
        result
    }

    #[test]
    fn sample_wav_preserves_signed_pcm_and_inclusive_loop_end() {
        let pcm = [0, 32512, 0, -32768, -256];
        let bytes = wav(
            &pcm,
            1,
            8000,
            Some((2, 5)),
            Some((60, 0)),
            b"{}",
            &AtomicBool::new(false),
        )
        .unwrap();
        let chunks = chunks(&bytes);
        let data = chunks.iter().find(|(tag, _)| *tag == b"data").unwrap().1;
        assert_eq!(data, &[0, 0, 0, 127, 0, 0, 0, 128, 0, 255]);
        let smpl = chunks.iter().find(|(tag, _)| *tag == b"smpl").unwrap().1;
        assert_eq!(u32::from_le_bytes(smpl[44..48].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(smpl[48..52].try_into().unwrap()), 4);
    }

    #[test]
    fn unicode_provenance_round_trips_through_ascii_soundfont_text() {
        let value = json!({"name":"日本語 🎵"});
        let escaped = ascii_json(&value).unwrap();
        assert!(escaped.is_ascii());
        assert_eq!(serde_json::from_str::<Value>(&escaped).unwrap(), value);
    }

    #[test]
    fn fixed_pcm_rate_requires_one_unambiguous_matched_table_setting() {
        use super::super::tables::{
            SettingsFields, SongDialect, SongTableBoundary, SongTableInventory,
        };
        let (input, mut manifest) = fixture();
        let mut bytes = input.bytes.to_vec();
        super::super::test_support::put_word(&mut bytes, 0x800, 7 << 16);
        super::super::test_support::put_word(&mut bytes, 0x820, 8 << 16);
        let mut song = manifest.scan.candidates[0].clone();
        assert_eq!(infer_mixer_rate(&bytes, &manifest, &song).0, None);
        for (settings, table) in [(0x800, 0x900), (0x820, 0xA00)] {
            manifest.scan.song_tables.push(SongTableInventory {
                selector: crate::audio_discovery::test_support::rom_span(0x700, 2),
                settings: crate::audio_discovery::test_support::rom_span(settings, 12),
                dialect: SongDialect::Mp2k,
                settings_fields: SettingsFields {
                    sound_mode: crate::audio_discovery::test_support::rom_span(settings, 4),
                    player_count: crate::audio_discovery::test_support::rom_span(settings + 4, 4),
                    player_table_pointer: crate::audio_discovery::test_support::rom_span(
                        settings + 8,
                        4,
                    ),
                },
                table: crate::audio_discovery::test_support::rom_span(table, 8),
                entries: Vec::new(),
                boundary: SongTableBoundary::MediaEnd {
                    effective_offset: bytes.len() as u32,
                },
            });
            song.table_entries.push(super::super::SongTableReference {
                table_offset: table as u32,
                index: 0,
                entry: crate::audio_discovery::test_support::rom_span(table, 8),
                player: 0,
            });
            let rate = infer_mixer_rate(&bytes, &manifest, &song).0;
            assert_eq!(rate, if table == 0x900 { Some(21_024) } else { None });
        }
    }

    #[test]
    fn export_rejects_stale_rom_and_cancellation_before_creating_output() {
        let (mut input, manifest) = fixture();
        let path = test_path("wav");
        let mut changed = input.bytes.to_vec();
        changed[0] ^= 1;
        input.bytes = changed.into();
        let request = AudioExportRequest::prepare(
            &input,
            &manifest,
            0,
            ExportKind::song(SongFormat::SoundFont),
        )
        .unwrap();
        assert!(
            request
                .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
                .unwrap_err()
                .to_string()
                .contains("SHA-256")
        );
        assert!(!path.exists());
        let request = AudioExportRequest::prepare(
            &input,
            &manifest,
            0,
            ExportKind::song(SongFormat::SoundFont),
        )
        .unwrap();
        assert!(
            request
                .write_new(&path, &AtomicBool::new(true), &AtomicU32::new(0))
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        assert!(!path.exists());
    }

    #[test]
    fn mapped_bundle_entries_match_rom_bytes_and_hashes() {
        let (input, manifest) = fixture();
        let path = test_path("zip");
        AudioExportRequest::prepare(
            &input,
            &manifest,
            0,
            ExportKind::song(SongFormat::MappedAssets),
        )
        .unwrap()
        .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
        .unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        let metadata: Value =
            serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
        for entry in metadata["entries"].as_array().unwrap() {
            let mut bytes = Vec::new();
            archive
                .by_name(entry["path"].as_str().unwrap())
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let start = entry["span"]["effective_offset"].as_u64().unwrap() as usize;
            assert_eq!(bytes, input.bytes[start..start + bytes.len()]);
            assert_eq!(entry["sha256"], zeff_firmware::sha256_hex(&bytes));
        }
    }
}
