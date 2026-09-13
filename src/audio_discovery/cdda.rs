use crate::audio_discovery::formats::FormatAvailability;
use std::io::{Seek, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
#[cfg(test)]
use zeff_pce_core::hardware::CD_RAW_SECTOR_BYTES;
use zeff_pce_core::hardware::{CdDisc, CdTrackMode};

use super::audio_file::{self, AudioInfo};
use super::formats::AudioFormat;

pub(crate) mod preview;
mod source;
pub(crate) use source::input_for_song;

pub(crate) use zeff_audio_discovery::cdda::CDDA_SAMPLE_RATE;
const CDDA_FRAMES_PER_SECTOR: u64 = zeff_pce_core::hardware::CD_AUDIO_FRAMES_PER_SECTOR as u64;
const MAX_TRACK_PCM_BYTES: u64 = 2 * 1024 * 1024 * 1024 - 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CdAudioProvenance {
    pub(crate) source_kind: &'static str,
    pub(crate) source_media_sha256: String,
    pub(crate) source_media_len: usize,
    pub(crate) selected_member_path_sha256: Option<String>,
    pub(crate) transforms_applied: bool,
}

#[derive(Clone)]
pub(crate) struct CdAudioInput {
    pub(crate) disc: Arc<CdDisc>,
    pub(crate) original_disc_sha256: String,
    pub(crate) original_disc_len: Option<usize>,
    pub(crate) effective_disc_sha256: String,
    pub(crate) effective_disc_len: usize,
    pub(crate) provenance: CdAudioProvenance,
}

impl CdAudioInput {
    pub(crate) fn new(
        disc: Arc<CdDisc>,
        original_disc_sha256: String,
        original_disc_len: Option<usize>,
        effective_disc_sha256: String,
        provenance: CdAudioProvenance,
    ) -> Result<Self> {
        ensure!(
            const_hex::encode(disc.content_hash()) == effective_disc_sha256,
            "CD audio input does not match the loaded effective disc identity"
        );
        let effective_disc_len = disc
            .payload_len()
            .context("loaded effective CD is too large")?;
        ensure!(
            effective_disc_len != 0,
            "loaded effective CD has no payload bytes"
        );
        Ok(Self {
            disc,
            original_disc_sha256,
            original_disc_len,
            effective_disc_sha256,
            effective_disc_len,
            provenance,
        })
    }

    pub(crate) fn tracks(&self) -> Result<Vec<CdAudioTrack>> {
        self.disc
            .tracks()
            .iter()
            .filter(|track| {
                track.mode() == CdTrackMode::Audio && track.end_lba() != track.index1_lba()
            })
            .map(track_inventory)
            .collect()
    }

    pub(crate) fn track(&self, number: u8) -> Result<CdAudioTrack> {
        self.disc
            .track(number)
            .filter(|track| track.mode() == CdTrackMode::Audio)
            .context("select an audio CD track before exporting")
            .and_then(track_inventory)
    }
}

pub(crate) use zeff_audio_discovery::cdda::CdAudioTrack;

fn track_inventory(track: &zeff_pce_core::hardware::CdTrack) -> Result<CdAudioTrack> {
    let sectors = track
        .end_lba()
        .checked_sub(track.index1_lba())
        .context("CD audio track ends before index 1")?;
    ensure!(sectors != 0, "CD audio track has no index-1 sectors");
    Ok(CdAudioTrack {
        number: track.number(),
        index1_lba: track.index1_lba(),
        end_lba: track.end_lba(),
        pregap_start_lba: track
            .index0_lba()
            .filter(|start| *start < track.index1_lba()),
        sectors,
        pcm_frames: u64::from(sectors) * CDDA_FRAMES_PER_SECTOR,
    })
}

pub(crate) fn write_new(
    input: &CdAudioInput,
    number: u8,
    format: AudioFormat,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    ensure!(format.available(), "selected audio format is unavailable");
    let track = input.track(number)?;
    ensure!(
        track.pcm_frames * 4 <= MAX_TRACK_PCM_BYTES,
        "CD audio track exceeds the PCM size limit"
    );
    ensure!(
        const_hex::encode(input.disc.content_hash()) == input.effective_disc_sha256,
        "loaded effective CD identity changed before export"
    );
    let metadata = serde_json::to_vec(&serde_json::json!({
        "schema": "zeff-cd-audio-export/1",
        "original_disc_sha256": input.original_disc_sha256.as_str(),
        "original_disc_len": input.original_disc_len,
        "effective_disc_sha256": input.effective_disc_sha256.as_str(),
        "effective_disc_len": input.effective_disc_len,
        "provenance": &input.provenance,
        "track": track,
        "pregap": "The output begins at index 1. Stored index-0 sectors are hashed for source verification but omitted from PCM.",
    }))?;
    crate::platform::write_new_file_atomically_streamed(
        path,
        |output| {
            let mut spool = spool_track(input, track, cancel)?;
            audio_file::encode_to(
                format,
                &mut spool,
                AudioInfo {
                    frames: track.pcm_frames,
                    channels: 2,
                    sample_rate: CDDA_SAMPLE_RATE,
                    loop_range: None,
                    pitch: None,
                },
                &metadata,
                cancel,
                output,
            )
        },
        || check_cancel(cancel),
    )
}

fn spool_track(
    input: &CdAudioInput,
    selection: CdAudioTrack,
    cancel: &AtomicBool,
) -> Result<std::fs::File> {
    let track = input
        .disc
        .track(selection.number)
        .context("selected CD audio track disappeared")?;
    ensure!(
        track.mode() == CdTrackMode::Audio
            && track.index1_lba() == selection.index1_lba
            && track.end_lba() == selection.end_lba,
        "selected CD audio track no longer matches the loaded table of contents"
    );
    let mut spool = tempfile::tempfile().context("could not create temporary CD audio storage")?;
    let mut hash = Sha256::new();
    for lba in track.stored_start_lba()..track.end_lba() {
        check_cancel(cancel)?;
        let sector = input.disc.read_audio_sector(lba)?;
        hash.update(sector);
        if lba >= track.index1_lba() {
            spool.write_all(&sector)?;
        }
    }
    ensure!(
        <[u8; 32]>::from(hash.finalize()) == track.payload_hash(),
        "CD audio source drifted after the disc was loaded"
    );
    spool.rewind()?;
    Ok(spool)
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "export cancelled");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use zeff_pce_core::hardware::{CdSourceError, CdTrack, CdTrackSource};

    fn input(disc: CdDisc) -> CdAudioInput {
        let disc = Arc::new(disc);
        CdAudioInput::new(
            Arc::clone(&disc),
            "01".repeat(32),
            Some(1),
            const_hex::encode(disc.content_hash()),
            CdAudioProvenance {
                source_kind: "synthetic",
                source_media_sha256: "02".repeat(32),
                source_media_len: 3,
                selected_member_path_sha256: None,
                transforms_applied: false,
            },
        )
        .unwrap()
    }

    fn sector(left: i16, right: i16) -> Vec<u8> {
        let mut data = Vec::with_capacity(CD_RAW_SECTOR_BYTES);
        for _ in 0..588 {
            data.extend(left.to_le_bytes());
            data.extend(right.to_le_bytes());
        }
        data
    }

    #[test]
    fn inventory_uses_index1_and_excludes_data_tracks() {
        let disc = CdDisc::new(vec![
            CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, vec![0; 2048])
                .unwrap(),
            CdTrack::from_stored_data(
                2,
                0,
                Some(1),
                2,
                CdTrackMode::Audio,
                [sector(1, -1), sector(2, -2)].concat(),
            )
            .unwrap(),
        ])
        .unwrap();
        let tracks = input(disc).tracks().unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].number, 2);
        assert_eq!(tracks[0].pregap_start_lba, Some(1));
        assert_eq!(tracks[0].index1_lba, 2);
        assert_eq!(tracks[0].sectors, 1);
        assert_eq!(tracks[0].pcm_frames, 588);
    }

    #[test]
    fn inventory_skips_zero_length_program_and_keeps_later_audio() {
        let disc = CdDisc::new(vec![
            CdTrack::from_stored_data(1, 0, Some(0), 1, CdTrackMode::Audio, sector(1, -1)).unwrap(),
            CdTrack::from_index1_data(2, 0, None, 2, CdTrackMode::Audio, sector(2, -2)).unwrap(),
        ])
        .unwrap();
        let input = input(disc);

        assert_eq!(
            input.tracks().unwrap().as_slice(),
            &[CdAudioTrack {
                number: 2,
                index1_lba: 2,
                end_lba: 3,
                pregap_start_lba: None,
                sectors: 1,
                pcm_frames: 588,
            }]
        );
        assert!(input.track(1).is_err());
    }

    #[test]
    fn wav_export_preserves_stereo_sector_bytes_and_skips_pregap() -> Result<()> {
        let pregap = sector(0x1111, -0x1111);
        let program = sector(0x2222, -0x2222);
        let disc = CdDisc::new(vec![
            CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, vec![0; 2048])
                .unwrap(),
            CdTrack::from_stored_data(
                2,
                0,
                Some(1),
                2,
                CdTrackMode::Audio,
                [pregap, program.clone()].concat(),
            )
            .unwrap(),
        ])
        .unwrap();
        let directory = crate::test_support::test_directory("cdda-wav")?;
        let output = directory.path().join("track.wav");
        write_new(
            &input(disc),
            2,
            AudioFormat::Wav,
            &output,
            &AtomicBool::new(false),
        )?;
        let bytes = std::fs::read(output)?;
        assert!(bytes.ends_with(&program));
        assert!(!bytes.ends_with(&[0x11, 0x11, 0xEF, 0xEE]));
        Ok(())
    }

    struct MutableSource(Mutex<Vec<u8>>);

    impl CdTrackSource for MutableSource {
        fn len(&self) -> usize {
            self.0.lock().unwrap().len()
        }

        fn payload_hash(&self) -> [u8; 32] {
            Sha256::digest(&*self.0.lock().unwrap()).into()
        }

        fn read_exact_at(&self, offset: usize, buffer: &mut [u8]) -> Result<(), CdSourceError> {
            let data = self.0.lock().unwrap();
            let end = offset
                .checked_add(buffer.len())
                .filter(|end| *end <= data.len())
                .ok_or(CdSourceError::ReadFailed)?;
            buffer.copy_from_slice(&data[offset..end]);
            Ok(())
        }
    }

    #[test]
    fn export_rejects_source_drift_cancellation_and_existing_output() -> Result<()> {
        let source = Arc::new(MutableSource(Mutex::new(sector(3, -3))));
        let track = CdTrack::from_index1_source(1, 0, None, 0, CdTrackMode::Audio, source.clone())?;
        let drift_input = input(CdDisc::new(vec![track])?);
        source.0.lock().unwrap()[0] ^= 1;
        let directory = crate::test_support::test_directory("cdda-reject")?;
        let drift = directory.path().join("drift.wav");
        assert!(
            write_new(
                &drift_input,
                1,
                AudioFormat::Wav,
                &drift,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(!drift.exists());

        let clean = input(CdDisc::new(vec![
            CdTrack::from_index1_data(1, 0, None, 0, CdTrackMode::Audio, sector(4, -4)).unwrap(),
        ])?);
        let cancelled = directory.path().join("cancelled.wav");
        let cancel = AtomicBool::new(true);
        assert!(write_new(&clean, 1, AudioFormat::Wav, &cancelled, &cancel).is_err());
        assert!(!cancelled.exists());

        let existing = directory.path().join("existing.wav");
        std::fs::write(&existing, b"existing")?;
        assert!(
            write_new(
                &clean,
                1,
                AudioFormat::Wav,
                &existing,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert_eq!(std::fs::read(existing)?, b"existing");
        Ok(())
    }
}
