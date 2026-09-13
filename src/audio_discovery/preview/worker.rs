use std::sync::{Arc, atomic::Ordering, mpsc::SyncSender};
use std::time::Duration;

use anyhow::{Result, ensure};

use super::{
    PreviewRequest, Shared,
    output::{self, Frame, OutputMode},
};

struct SilenceOnExit<'a>(&'a Shared);

impl Drop for SilenceOnExit<'_> {
    fn drop(&mut self) {
        self.0.playing.store(false, Ordering::Release);
    }
}

struct PendingPcm {
    pcm: [i16; 512],
    start: usize,
    frames: usize,
    next: usize,
}

impl PendingPcm {
    fn new(position: usize) -> Self {
        Self {
            pcm: [0; 512],
            start: position,
            frames: 0,
            next: 0,
        }
    }

    fn position(&self) -> usize {
        self.start + self.next
    }

    fn is_empty(&self) -> bool {
        self.next == self.frames
    }

    fn drain(
        &mut self,
        producer: &mut rtrb::Producer<Frame>,
        shared: &Shared,
        generation: u32,
    ) -> Result<()> {
        let end = self.frames.min(self.next + producer.slots());
        for index in self.next..end {
            if shared.cancel.load(Ordering::Acquire) || shared.generation() != generation {
                break;
            }
            producer
                .push(Frame {
                    generation: u64::from(generation),
                    position: self.start + index,
                    pcm: [self.pcm[index * 2], self.pcm[index * 2 + 1]],
                })
                .map_err(|_| {
                    anyhow::anyhow!("preview audio queue capacity changed unexpectedly")
                })?;
            self.next += 1;
        }
        Ok(())
    }
}

pub(super) fn run(
    request: PreviewRequest,
    output: OutputMode,
    shared: &Arc<Shared>,
    warnings: SyncSender<Vec<String>>,
) -> Result<()> {
    let _silence = SilenceOnExit(shared);
    let (_stream, mut producer, rate) = output::open(shared, output)?;
    let mut renderer = request.renderer(rate, &shared.cancel)?;
    shared
        .duration
        .store(renderer.duration_frames(), Ordering::Release);
    shared
        .sample_rate
        .store(renderer.sample_rate(), Ordering::Release);
    shared
        .tracks
        .store(renderer.track_count(), Ordering::Release);
    let _ = warnings.send(renderer.warnings().to_vec());
    let mut generation = 0;
    let track_bits = u16::MAX >> (16 - renderer.track_count());
    let mut pending = PendingPcm::new(0);
    loop {
        if shared.cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        ensure!(
            !shared.device_error.load(Ordering::Acquire),
            "audio output device stopped; play again to reopen the current default device"
        );
        let command = *shared
            .command
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if generation != command.generation {
            if !renderer.seek_direct(command.target)? {
                renderer.reset()?;
                renderer.set_track_mask(shared.mask.load(Ordering::Acquire) & track_bits)?;
                while renderer.position_frames() < command.target {
                    if shared.cancel.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    if shared.generation() != command.generation {
                        break;
                    }
                    let frames = (command.target - renderer.position_frames())
                        .min(pending.pcm.len() / 2)
                        .max(8);
                    renderer.read(&mut pending.pcm[..frames * 2], &shared.cancel)?;
                }
            }
            if shared.generation() != command.generation {
                continue;
            }
            generation = command.generation;
            pending = PendingPcm::new(renderer.position_frames());
        }
        renderer.set_track_mask(shared.mask.load(Ordering::Acquire) & track_bits)?;
        let cursor = shared.cursor.load(Ordering::Acquire);
        if (cursor >> 32) as u32 != generation {
            continue;
        }
        let consumed = cursor as u32 as usize;
        let queued = pending.position().saturating_sub(consumed);
        let target = shared
            .preroll_frames()
            .min(renderer.duration_frames().saturating_sub(consumed));
        if queued >= target {
            shared.ready.store(generation, Ordering::Release);
        }
        if queued >= target
            || producer.slots() == 0
            || (pending.is_empty() && renderer.position_frames() == renderer.duration_frames())
        {
            std::thread::park_timeout(Duration::from_millis(2));
            continue;
        }
        if pending.is_empty() {
            pending.start = renderer.position_frames();
            pending.frames = renderer.read(&mut pending.pcm, &shared.cancel)? / 2;
            pending.next = 0;
        }
        // Device callbacks need not consume a whole synthesis block.
        pending.drain(&mut producer, shared, generation)?;
    }
}

#[cfg(test)]
mod tests;
