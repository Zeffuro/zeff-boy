use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};

use super::{
    CDDA_FRAMES_PER_SECTOR, CDDA_SAMPLE_RATE, CdAudioInput, CdAudioTrack, MAX_TRACK_PCM_BYTES,
    spool_track,
};
use crate::audio_discovery::render::validate_sample_rate;

const SOURCE_CACHE_FRAMES: usize = 4_096;

pub(crate) struct NativeRenderSession {
    spool: std::fs::File,
    source_frames: usize,
    cache: Vec<u8>,
    cache_start: usize,
    cache_frames: usize,
    duration_frames: usize,
    sample_rate: u32,
    position_frames: usize,
    track_mask: u16,
    warnings: Vec<String>,
}

impl NativeRenderSession {
    pub(crate) fn new(
        input: &CdAudioInput,
        selection: CdAudioTrack,
        sample_rate: u32,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        check_cancelled(cancel)?;
        validate_sample_rate(sample_rate)?;
        ensure!(
            selection.pcm_frames != 0,
            "CD audio track has no index-1 frames"
        );
        ensure!(
            selection.pcm_frames == u64::from(selection.sectors) * CDDA_FRAMES_PER_SECTOR,
            "CD audio track has inconsistent index-1 geometry"
        );
        ensure!(
            selection.pcm_frames * 4 <= MAX_TRACK_PCM_BYTES,
            "CD audio track exceeds the PCM size limit"
        );
        ensure!(
            input.track(selection.number)? == selection,
            "selected CD audio track no longer matches the loaded table of contents"
        );
        ensure!(
            const_hex::encode(input.disc.content_hash()) == input.effective_disc_sha256,
            "loaded effective CD identity changed before preview"
        );
        let source_frames = usize::try_from(selection.pcm_frames)
            .context("CD audio track duration does not fit this platform")?;
        let duration_frames = scaled_frames(source_frames, sample_rate)?;
        let spool = spool_track(input, selection, cancel)?;
        check_cancelled(cancel)?;
        let warnings = (sample_rate != CDDA_SAMPLE_RATE)
            .then(|| {
                "CDDA preview uses deterministic linear interpolation when the output rate differs from 44.1 kHz.".to_owned()
            })
            .into_iter()
            .collect();
        Ok(Self {
            spool,
            source_frames,
            cache: vec![0; SOURCE_CACHE_FRAMES * 4],
            cache_start: 0,
            cache_frames: 0,
            duration_frames,
            sample_rate,
            position_frames: 0,
            track_mask: 1,
            warnings,
        })
    }

    pub(crate) fn duration_frames(&self) -> usize {
        self.duration_frames
    }

    pub(crate) fn position_frames(&self) -> usize {
        self.position_frames
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn track_count(&self) -> usize {
        1
    }

    pub(crate) fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub(crate) fn reset(&mut self) -> Result<()> {
        self.spool.rewind()?;
        self.cache_frames = 0;
        self.position_frames = 0;
        Ok(())
    }

    pub(crate) fn seek_frames(&mut self, frame: usize) -> Result<()> {
        self.position_frames = frame.min(self.duration_frames);
        Ok(())
    }

    pub(crate) fn set_track_mask(&mut self, track_mask: u16) -> Result<()> {
        ensure!(
            track_mask & !1 == 0,
            "track mask selects an unavailable track"
        );
        self.track_mask = track_mask;
        Ok(())
    }

    pub(crate) fn read(&mut self, output: &mut [i16], cancel: &AtomicBool) -> Result<usize> {
        ensure!(
            output.len().is_multiple_of(2),
            "render read buffer must contain complete stereo frames"
        );
        check_cancelled(cancel)?;
        let frames = (output.len() / 2).min(self.duration_frames - self.position_frames);
        for frame in 0..frames {
            check_cancelled(cancel)?;
            let source = self.frame_at(self.position_frames + frame)?;
            let target = &mut output[frame * 2..frame * 2 + 2];
            if self.track_mask == 0 {
                target.fill(0);
            } else {
                target.copy_from_slice(&source);
            }
        }
        self.position_frames += frames;
        Ok(frames * 2)
    }

    fn frame_at(&mut self, output_frame: usize) -> Result<[i16; 2]> {
        if self.sample_rate == CDDA_SAMPLE_RATE {
            return self.read_source_frame(output_frame);
        }
        let numerator = (output_frame as u128) * u128::from(CDDA_SAMPLE_RATE);
        let source_index = usize::try_from(numerator / u128::from(self.sample_rate))
            .context("CDDA resample position overflows")?;
        let fraction = u64::try_from(numerator % u128::from(self.sample_rate))
            .context("CDDA resample fraction overflows")?;
        let first = self.read_source_frame(source_index)?;
        if fraction == 0 || source_index + 1 >= self.source_frames {
            return Ok(first);
        }
        let second = self.read_source_frame(source_index + 1)?;
        Ok([
            interpolate(first[0], second[0], fraction, self.sample_rate),
            interpolate(first[1], second[1], fraction, self.sample_rate),
        ])
    }

    fn read_source_frame(&mut self, frame: usize) -> Result<[i16; 2]> {
        ensure!(
            frame < self.source_frames,
            "CDDA source frame is out of range"
        );
        if frame < self.cache_start || frame >= self.cache_start + self.cache_frames {
            self.cache_start = frame / SOURCE_CACHE_FRAMES * SOURCE_CACHE_FRAMES;
            self.cache_frames = (self.source_frames - self.cache_start).min(SOURCE_CACHE_FRAMES);
            let offset = u64::try_from(self.cache_start)
                .context("CDDA source position overflows")?
                .checked_mul(4)
                .context("CDDA source offset overflows")?;
            self.spool.seek(SeekFrom::Start(offset))?;
            self.spool
                .read_exact(&mut self.cache[..self.cache_frames * 4])?;
        }
        let offset = (frame - self.cache_start) * 4;
        let bytes = &self.cache[offset..offset + 4];
        Ok([
            i16::from_le_bytes([bytes[0], bytes[1]]),
            i16::from_le_bytes([bytes[2], bytes[3]]),
        ])
    }
}

fn scaled_frames(source_frames: usize, sample_rate: u32) -> Result<usize> {
    let frames = (source_frames as u128)
        .checked_mul(u128::from(sample_rate))
        .context("CDDA preview duration overflows")?
        .checked_add(u128::from(CDDA_SAMPLE_RATE) - 1)
        .context("CDDA preview duration overflows")?
        / u128::from(CDDA_SAMPLE_RATE);
    usize::try_from(frames).context("CDDA preview duration does not fit this platform")
}

fn interpolate(first: i16, second: i16, numerator: u64, denominator: u32) -> i16 {
    let delta = i64::from(second) - i64::from(first);
    let scaled = delta * numerator as i64;
    let denominator = i64::from(denominator);
    let rounded = if scaled >= 0 {
        (scaled + denominator / 2) / denominator
    } else {
        (scaled - denominator / 2) / denominator
    };
    (i64::from(first) + rounded) as i16
}

fn check_cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "CDDA preview cancelled");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use sha2::{Digest, Sha256};
    use zeff_pce_core::hardware::{
        CD_RAW_SECTOR_BYTES, CdDisc, CdSourceError, CdTrack, CdTrackMode, CdTrackSource,
    };

    use super::*;
    use crate::audio_discovery::cdda::CdAudioProvenance;

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

    fn sector(samples: &[[i16; 2]]) -> Vec<u8> {
        assert_eq!(samples.len(), 588);
        samples
            .iter()
            .flat_map(|[left, right]| [left.to_le_bytes(), right.to_le_bytes()])
            .flatten()
            .collect()
    }

    fn repeated(left: i16, right: i16) -> Vec<u8> {
        sector(&vec![[left, right]; 588])
    }

    fn samples(bytes: &[u8]) -> Vec<i16> {
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|sample| i16::from_le_bytes(*sample))
            .collect()
    }

    fn session(input: &CdAudioInput, number: u8, rate: u32) -> NativeRenderSession {
        NativeRenderSession::new(
            input,
            input.track(number).unwrap(),
            rate,
            &AtomicBool::new(false),
        )
        .unwrap()
    }

    #[test]
    fn exact_44100_preserves_index1_stereo_across_sector_boundaries() -> Result<()> {
        let pregap = repeated(111, -111);
        let first = sector(
            &(0..588)
                .map(|frame| [frame as i16, -(frame as i16)])
                .collect::<Vec<_>>(),
        );
        let second = sector(
            &(0..588)
                .map(|frame| [2_000 + frame as i16, -2_000 - frame as i16])
                .collect::<Vec<_>>(),
        );
        let disc = CdDisc::new(vec![CdTrack::from_stored_data(
            1,
            0,
            Some(1),
            2,
            CdTrackMode::Audio,
            [pregap, first.clone(), second.clone()].concat(),
        )?])?;
        let input = input(disc);
        let mut renderer = session(&input, 1, CDDA_SAMPLE_RATE);
        let mut output = vec![0; (first.len() + second.len()) / 2];
        assert_eq!(
            renderer.read(&mut output, &AtomicBool::new(false))?,
            output.len()
        );
        assert_eq!(output, samples(&[first, second].concat()));
        assert_eq!(renderer.duration_frames(), 1_176);
        assert_eq!(renderer.position_frames(), 1_176);
        assert!(renderer.warnings().is_empty());
        Ok(())
    }

    #[test]
    fn rational_resampling_is_chunk_independent_resettable_and_seekable() -> Result<()> {
        let mut pcm = Vec::new();
        for frame in 0..588 {
            let value = match frame {
                0 => 0,
                1 => 10_000,
                _ => -10_000,
            };
            pcm.extend([value, -value]);
        }
        let source = sector(
            &pcm.as_chunks::<2>()
                .0
                .iter()
                .map(|frame| [frame[0], frame[1]])
                .collect::<Vec<_>>(),
        );
        let input = input(CdDisc::new(vec![CdTrack::from_index1_data(
            1,
            0,
            None,
            0,
            CdTrackMode::Audio,
            source,
        )?])?);
        let cancel = AtomicBool::new(false);
        let mut whole = session(&input, 1, 48_000);
        let mut expected = vec![0; whole.duration_frames() * 2];
        assert_eq!(whole.read(&mut expected, &cancel)?, expected.len());
        assert_eq!(whole.duration_frames(), 640);
        assert_eq!(
            &expected[..8],
            &[0, 0, 9_188, -9_188, -6_750, 6_750, -10_000, 10_000]
        );
        assert_eq!(expected[expected.len() - 2..], [-10_000, 10_000]);
        assert_eq!(session(&input, 1, 63_072).duration_frames(), 841);
        assert_eq!(session(&input, 1, 96_000).duration_frames(), 1_280);

        let mut chunked = session(&input, 1, 48_000);
        let mut actual = Vec::new();
        for frames in [1, 17, 3, 221, 398] {
            let mut block = vec![0; frames * 2];
            let samples = chunked.read(&mut block, &cancel)?;
            actual.extend_from_slice(&block[..samples]);
        }
        assert_eq!(actual, expected);
        chunked.reset()?;
        let mut replay = vec![0; expected.len()];
        chunked.read(&mut replay, &cancel)?;
        assert_eq!(replay, expected);
        chunked.seek_frames(2)?;
        let mut sought = vec![0; 12];
        assert_eq!(chunked.read(&mut sought, &cancel)?, sought.len());
        assert_eq!(sought, expected[4..16]);
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

    struct CancellingSource {
        data: Vec<u8>,
        cancel: Arc<AtomicBool>,
    }

    impl CdTrackSource for CancellingSource {
        fn len(&self) -> usize {
            self.data.len()
        }

        fn payload_hash(&self) -> [u8; 32] {
            Sha256::digest(&self.data).into()
        }

        fn read_exact_at(&self, offset: usize, buffer: &mut [u8]) -> Result<(), CdSourceError> {
            let end = offset
                .checked_add(buffer.len())
                .filter(|end| *end <= self.data.len())
                .ok_or(CdSourceError::ReadFailed)?;
            buffer.copy_from_slice(&self.data[offset..end]);
            self.cancel.store(true, Ordering::Relaxed);
            Ok(())
        }
    }

    #[test]
    fn constructor_rejects_drift_and_spool_isolated_from_later_mutation() -> Result<()> {
        let source = Arc::new(MutableSource(Mutex::new(
            [repeated(11, -11), repeated(23, -23)].concat(),
        )));
        let track =
            CdTrack::from_stored_source(1, 0, Some(0), 1, CdTrackMode::Audio, source.clone())?;
        let drifted = input(CdDisc::new(vec![track])?);
        source.0.lock().unwrap()[0] ^= 1;
        assert!(
            NativeRenderSession::new(
                &drifted,
                drifted.track(1)?,
                CDDA_SAMPLE_RATE,
                &AtomicBool::new(false)
            )
            .is_err()
        );

        let source = Arc::new(MutableSource(Mutex::new(repeated(37, -37))));
        let track = CdTrack::from_index1_source(1, 0, None, 0, CdTrackMode::Audio, source.clone())?;
        let clean = input(CdDisc::new(vec![track])?);
        let mut renderer = session(&clean, 1, CDDA_SAMPLE_RATE);
        source.0.lock().unwrap().fill(0);
        let mut output = [0; 4];
        renderer.read(&mut output, &AtomicBool::new(false))?;
        assert_eq!(output, [37, -37, 37, -37]);
        Ok(())
    }

    #[test]
    fn constructor_rejects_forged_track_geometry() -> Result<()> {
        let input = input(CdDisc::new(vec![CdTrack::from_stored_data(
            1,
            0,
            Some(1),
            2,
            CdTrackMode::Audio,
            [repeated(1, -1), repeated(2, -2), repeated(3, -3)].concat(),
        )?])?);
        let selection = input.track(1)?;

        let mut forged = selection;
        forged.pregap_start_lba = None;
        assert!(
            NativeRenderSession::new(&input, forged, CDDA_SAMPLE_RATE, &AtomicBool::new(false))
                .is_err()
        );

        let mut forged = selection;
        forged.sectors -= 1;
        forged.pcm_frames = u64::from(forged.sectors) * 588;
        assert!(
            NativeRenderSession::new(&input, forged, CDDA_SAMPLE_RATE, &AtomicBool::new(false))
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn constructor_observes_cancellation_during_the_final_source_read() -> Result<()> {
        let cancel = Arc::new(AtomicBool::new(false));
        let source = Arc::new(CancellingSource {
            data: repeated(9, -9),
            cancel: Arc::clone(&cancel),
        });
        let disc = CdDisc::new(vec![CdTrack::from_index1_source(
            1,
            0,
            None,
            0,
            CdTrackMode::Audio,
            source,
        )?])?;
        cancel.store(false, Ordering::Relaxed);
        let input = input(disc);
        assert!(NativeRenderSession::new(&input, input.track(1)?, 44_100, &cancel).is_err());
        Ok(())
    }

    #[test]
    fn cancellation_masks_and_invalid_buffers_are_bounded() -> Result<()> {
        let input = input(CdDisc::new(vec![CdTrack::from_index1_data(
            1,
            0,
            None,
            0,
            CdTrackMode::Audio,
            repeated(7, -7),
        )?])?);
        let cancelled = AtomicBool::new(true);
        assert!(NativeRenderSession::new(&input, input.track(1)?, 44_100, &cancelled).is_err());
        let mut renderer = session(&input, 1, 44_100);
        assert!(renderer.read(&mut [0; 3], &AtomicBool::new(false)).is_err());
        renderer.set_track_mask(0)?;
        let mut muted = [1; 8];
        assert_eq!(
            renderer.read(&mut muted, &AtomicBool::new(false))?,
            muted.len()
        );
        assert!(muted.iter().all(|sample| *sample == 0));
        assert_eq!(renderer.position_frames(), 4);
        assert!(renderer.set_track_mask(2).is_err());
        assert!(renderer.read(&mut muted, &AtomicBool::new(true)).is_err());
        assert!(
            NativeRenderSession::new(&input, input.track(1)?, 22_050, &AtomicBool::new(false))
                .is_err()
        );
        let mut empty = input.track(1)?;
        empty.pcm_frames = 0;
        assert!(NativeRenderSession::new(&input, empty, 44_100, &AtomicBool::new(false)).is_err());
        Ok(())
    }

    #[test]
    fn sector_shape_stays_valid() {
        assert_eq!(repeated(1, -1).len(), CD_RAW_SECTOR_BYTES);
    }
}
