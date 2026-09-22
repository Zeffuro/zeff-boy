use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::pcm::PcmSession;

pub(crate) mod reference;

const SILENCE_THRESHOLD: u16 = 8;
const MAX_ACTIVITY_INTERVALS: usize = 256;

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct Interval {
    pub(crate) start_frame: usize,
    pub(crate) end_frame: usize,
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct PcmEvidence {
    pub(crate) frames: usize,
    pub(crate) sample_rate: u32,
    pub(crate) pcm_sha256: String,
    nonzero_frames: usize,
    active_frames: usize,
    peak: f64,
    rms: f64,
    clipped_samples: usize,
    first_active_frame: Option<usize>,
    last_active_frame: Option<usize>,
    pub(crate) activity_intervals: Vec<Interval>,
    pub(crate) activity_intervals_truncated: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct PlaybackEvidence {
    pub(crate) pcm: PcmEvidence,
    pub(crate) fresh_render_matches: bool,
    pub(crate) reset_render_matches: bool,
    pub(crate) silent: bool,
    session_duration_frames: usize,
    pub(crate) validation_duration_capped: bool,
    pub(crate) silence_threshold_i16: u16,
    pub(crate) activity_gap_frames: usize,
    warnings: Vec<String>,
}

impl PlaybackEvidence {
    pub(crate) fn deterministic(&self) -> bool {
        self.fresh_render_matches && self.reset_render_matches
    }
}

pub(crate) fn validate(
    mut factory: impl FnMut() -> Result<Box<dyn PcmSession>>,
    max_frames: usize,
    cancel: &AtomicBool,
) -> Result<PlaybackEvidence> {
    ensure!(max_frames > 0, "validation duration is empty");
    let mut session = factory()?;
    let session_duration_frames = session.duration_frames();
    let first = render(&mut *session, max_frames, 1024, cancel)?;
    let warnings = session.warnings().to_vec();
    session.reset()?;
    let reset = render(&mut *session, max_frames, 257, cancel)?;
    drop(session);
    let fresh = render(&mut *factory()?, max_frames, 613, cancel)?;
    Ok(PlaybackEvidence {
        fresh_render_matches: first == fresh,
        reset_render_matches: first == reset,
        silent: first.active_frames == 0,
        validation_duration_capped: session_duration_frames > max_frames,
        session_duration_frames,
        silence_threshold_i16: SILENCE_THRESHOLD,
        activity_gap_frames: first.sample_rate as usize * 3 / 4,
        warnings,
        pcm: first,
    })
}

fn render(
    session: &mut dyn PcmSession,
    max_frames: usize,
    chunk_frames: usize,
    cancel: &AtomicBool,
) -> Result<PcmEvidence> {
    render_into(session, max_frames, chunk_frames, cancel, |_| Ok(()))
}

pub(super) fn render_into(
    session: &mut dyn PcmSession,
    max_frames: usize,
    chunk_frames: usize,
    cancel: &AtomicBool,
    mut sink: impl FnMut(&[i16]) -> Result<()>,
) -> Result<PcmEvidence> {
    ensure!(
        (8..=1024).contains(&chunk_frames),
        "invalid validation chunk size"
    );
    ensure!(
        session.position_frames() == 0,
        "validation session is not at its start"
    );
    super::render::validate_sample_rate(session.sample_rate())?;
    let expected = session.duration_frames().min(max_frames);
    ensure!(expected > 0, "validation session has no frames");
    let mut metrics = Metrics::new(session.sample_rate());
    let mut output = vec![0_i16; chunk_frames * 2];
    while metrics.frames < expected {
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "audio validation cancelled"
        );
        // The SoundFont renderer requires space for at least eight stereo frames.
        let requested = chunk_frames.min(expected - metrics.frames).max(8);
        let position = session.position_frames();
        let count = session.read(&mut output[..requested * 2], cancel)?;
        ensure!(
            count > 0 && count <= requested * 2 && count.is_multiple_of(2),
            "audio renderer returned invalid or premature PCM length"
        );
        let retained = count.min((expected - metrics.frames) * 2);
        metrics.push(&output[..retained]);
        sink(&output[..retained])?;
        ensure!(
            session.position_frames() == position + count / 2,
            "audio renderer position disagrees with returned PCM"
        );
    }
    if expected == session.duration_frames() {
        ensure!(
            session.read(&mut output[..16], cancel)? == 0,
            "audio renderer exceeded its declared duration"
        );
    }
    Ok(metrics.finish())
}

struct Metrics {
    hash: Sha256,
    frames: usize,
    rate: u32,
    nonzero: usize,
    active: usize,
    peak: u16,
    square_sum: u128,
    clipped: usize,
    first: Option<usize>,
    last: Option<usize>,
    interval_start: Option<usize>,
    intervals: Vec<Interval>,
    truncated: bool,
}

impl Metrics {
    fn new(rate: u32) -> Self {
        Self {
            hash: Sha256::new(),
            frames: 0,
            rate,
            nonzero: 0,
            active: 0,
            peak: 0,
            square_sum: 0,
            clipped: 0,
            first: None,
            last: None,
            interval_start: None,
            intervals: Vec::new(),
            truncated: false,
        }
    }

    fn push(&mut self, pcm: &[i16]) {
        for frame in pcm.as_chunks::<2>().0 {
            let peak = frame[0].unsigned_abs().max(frame[1].unsigned_abs());
            self.nonzero += usize::from(peak != 0);
            self.peak = self.peak.max(peak);
            if peak > SILENCE_THRESHOLD {
                if let Some(last) = self.last
                    && self.frames - last > self.rate as usize * 3 / 4
                {
                    self.close_interval();
                }
                self.first.get_or_insert(self.frames);
                self.interval_start.get_or_insert(self.frames);
                self.last = Some(self.frames);
                self.active += 1;
            }
            for &sample in frame {
                self.hash.update(sample.to_le_bytes());
                self.square_sum += u128::from(sample.unsigned_abs()).pow(2);
                self.clipped += usize::from(sample == i16::MIN || sample == i16::MAX);
            }
            self.frames += 1;
        }
    }

    fn close_interval(&mut self) {
        if let (Some(start), Some(last)) = (self.interval_start.take(), self.last) {
            if self.intervals.len() < MAX_ACTIVITY_INTERVALS {
                self.intervals.push(Interval {
                    start_frame: start,
                    end_frame: last + 1,
                });
            } else {
                self.truncated = true;
            }
        }
    }

    fn finish(mut self) -> PcmEvidence {
        self.close_interval();
        PcmEvidence {
            frames: self.frames,
            sample_rate: self.rate,
            pcm_sha256: const_hex::encode(self.hash.finalize()),
            nonzero_frames: self.nonzero,
            active_frames: self.active,
            peak: f64::from(self.peak) / 32768.0,
            rms: (self.square_sum as f64 / (self.frames as f64 * 2.0)).sqrt() / 32768.0,
            clipped_samples: self.clipped,
            first_active_frame: self.first,
            last_active_frame: self.last,
            activity_intervals: self.intervals,
            activity_intervals_truncated: self.truncated,
        }
    }
}

#[cfg(test)]
mod tests;
