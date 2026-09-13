use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    mpsc::{self, Receiver},
};
use std::thread::JoinHandle;

use anyhow::Result;

mod output;
mod request;
#[cfg(test)]
mod tests;
mod worker;

pub(crate) use request::PreviewRequest;

pub(crate) const DEFAULT_VOLUME_PERCENT: u32 = 70;
pub(crate) const MAX_PREVIEW_SECONDS: u16 = 30 * 60;

#[derive(Clone, Copy)]
struct Command {
    generation: u32,
    target: usize,
}

struct Shared {
    seek_step: usize,
    cancel: AtomicBool,
    playing: AtomicBool,
    cursor: AtomicU64,
    ready: AtomicU32,
    duration: AtomicUsize,
    sample_rate: AtomicU32,
    tracks: AtomicUsize,
    mask: AtomicU16,
    volume: AtomicU32,
    device_error: AtomicBool,
    queue_capacity: AtomicUsize,
    callback_frames: AtomicUsize,
    underruns: AtomicU64,
    command: Mutex<Command>,
}

impl Shared {
    fn new() -> Self {
        Self {
            seek_step: 8,
            cancel: AtomicBool::new(false),
            playing: AtomicBool::new(true),
            cursor: AtomicU64::new(1 << 32),
            ready: AtomicU32::new(0),
            duration: AtomicUsize::new(0),
            sample_rate: AtomicU32::new(super::render::DEFAULT_SAMPLE_RATE),
            tracks: AtomicUsize::new(0),
            mask: AtomicU16::new(u16::MAX),
            volume: AtomicU32::new(DEFAULT_VOLUME_PERCENT),
            device_error: AtomicBool::new(false),
            queue_capacity: AtomicUsize::new(output::queue_frames(
                super::render::DEFAULT_SAMPLE_RATE,
            )),
            callback_frames: AtomicUsize::new(0),
            underruns: AtomicU64::new(0),
            command: Mutex::new(Command {
                generation: 1,
                target: 0,
            }),
        }
    }

    fn generation(&self) -> u32 {
        (self.cursor.load(Ordering::Acquire) >> 32) as u32
    }

    fn preroll_frames(&self) -> usize {
        (self.sample_rate.load(Ordering::Relaxed) as usize / 20)
            .max(
                self.callback_frames
                    .load(Ordering::Relaxed)
                    .saturating_mul(2),
            )
            .min(self.queue_capacity.load(Ordering::Relaxed))
    }

    fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
        self.playing.store(false, Ordering::Release);
    }
}

struct Job {
    shared: Arc<Shared>,
    thread: JoinHandle<Result<()>>,
    warnings: Receiver<Vec<String>>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviewSnapshot {
    pub(crate) playing: bool,
    pub(crate) preparing: bool,
    pub(crate) position: usize,
    pub(crate) duration: usize,
    pub(crate) sample_rate: u32,
    pub(crate) tracks: usize,
}

#[derive(Default)]
pub(crate) struct PreviewPlayer {
    job: Option<Job>,
    queued: Option<(PreviewRequest, output::OutputMode)>,
    pub(crate) error: Option<String>,
    pub(crate) warnings: Vec<String>,
    mask: Option<u16>,
    volume: Option<u32>,
}

impl PreviewPlayer {
    #[cfg(test)]
    pub(crate) fn start_captured(&mut self, request: PreviewRequest) -> Receiver<output::Callback> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.start_with_output(request, output::OutputMode::Capture(sender));
        receiver
    }

    pub(crate) fn start(&mut self, request: PreviewRequest) {
        self.start_with_output(request, output::OutputMode::Device);
    }

    fn start_with_output(&mut self, request: PreviewRequest, output: output::OutputMode) {
        self.stop();
        self.error = None;
        self.warnings.clear();
        self.queued = Some((request, output));
        self.poll();
    }

    pub(crate) fn stop(&mut self) {
        self.queued = None;
        if let Some(job) = &self.job {
            job.shared.stop();
        }
    }

    pub(crate) fn poll(&mut self) {
        if let Some(job) = &self.job
            && !job.shared.cancel.load(Ordering::Acquire)
            && let Ok(warnings) = job.warnings.try_recv()
        {
            self.warnings = warnings;
        }
        if self
            .job
            .as_ref()
            .is_some_and(|job| job.thread.is_finished())
        {
            let job = self.job.take().unwrap();
            let cancelled = job.shared.cancel.load(Ordering::Acquire);
            match job.thread.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) if !cancelled => {
                    self.error = Some(format!("Preview failed: {error:#}"))
                }
                Err(_) if !cancelled => {
                    self.error = Some("Preview worker stopped unexpectedly".into())
                }
                _ => {}
            }
        }
        if self.job.is_none()
            && let Some((request, output)) = self.queued.take()
        {
            let shared = Arc::new(Shared {
                seek_step: request.seek_step(),
                ..Shared::new()
            });
            shared
                .mask
                .store(self.mask.unwrap_or(u16::MAX), Ordering::Relaxed);
            shared
                .volume
                .store(self.volume.unwrap_or(70), Ordering::Relaxed);
            let worker_shared = Arc::clone(&shared);
            let (sender, warnings) = mpsc::sync_channel(1);
            match std::thread::Builder::new()
                .name("zeff-audio-preview".into())
                .spawn(move || worker::run(request, output, &worker_shared, sender))
            {
                Ok(thread) => {
                    self.job = Some(Job {
                        shared,
                        thread,
                        warnings,
                    })
                }
                Err(error) => self.error = Some(format!("Could not start preview: {error}")),
            }
        }
    }

    pub(crate) fn snapshot(&self) -> Option<PreviewSnapshot> {
        let shared = &self.job.as_ref()?.shared;
        if shared.cancel.load(Ordering::Acquire) {
            return None;
        }
        let cursor = shared.cursor.load(Ordering::Acquire);
        let duration = shared.duration.load(Ordering::Acquire);
        let position = cursor as u32 as usize;
        Some(PreviewSnapshot {
            playing: shared.playing.load(Ordering::Acquire)
                && (duration == 0 || position < duration),
            preparing: shared.ready.load(Ordering::Acquire) != (cursor >> 32) as u32,
            position,
            duration,
            sample_rate: shared.sample_rate.load(Ordering::Acquire),
            tracks: shared.tracks.load(Ordering::Acquire),
        })
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.job.is_some() || self.queued.is_some()
    }

    pub(crate) fn set_playing(&mut self, playing: bool) {
        if playing
            && self
                .snapshot()
                .is_some_and(|state| state.duration != 0 && state.position >= state.duration)
        {
            self.seek(0);
        }
        if let Some(job) = &self.job {
            job.shared.playing.store(playing, Ordering::Release);
        }
    }

    pub(crate) fn seek(&mut self, frame: usize) {
        if let Some(job) = &self.job {
            let shared = &job.shared;
            let mut command = shared
                .command
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            command.generation = command.generation.wrapping_add(1).max(1);
            command.target = (frame / shared.seek_step * shared.seek_step)
                .min(shared.duration.load(Ordering::Acquire));
            shared.cursor.store(
                (u64::from(command.generation) << 32) | command.target as u64,
                Ordering::Release,
            );
        }
    }

    pub(crate) fn set_track_mask(&mut self, mask: u16) {
        self.mask = Some(mask);
        if let Some(job) = &self.job {
            job.shared.mask.store(mask, Ordering::Release);
        }
    }

    pub(crate) fn set_volume(&mut self, volume: u32) {
        self.volume = Some(volume.min(100));
        if let Some(job) = &self.job {
            job.shared.volume.store(volume.min(100), Ordering::Relaxed);
        }
    }
}

impl Drop for PreviewPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}
