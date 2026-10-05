use super::*;
use zeff_netplay::rollback::FrameInput;

pub(super) struct FrameBucket {
    ports: [u16; 2],
    audio: Vec<f32>,
    checkpoint: Option<Message>,
}

impl Session {
    pub(super) fn execute(&mut self, backend: &mut EmuBackend, input: FrameInput) -> Result<()> {
        ensure!(
            backend.frame_count() == input.frame,
            "rollback core frame differs"
        );
        let audio = self.lease.advance(backend, input.ports)?;
        let completed = input.frame.checked_add(1).context("frame overflow")?;
        let snapshot = self.lease.capture(backend)?;
        ensure!(
            snapshot.frame() == completed,
            "rollback snapshot frame differs"
        );
        let checkpoint = if self.verification || completed % checks::HASH_INTERVAL == 0 {
            Some(identity::checkpoint_with_snapshot(
                backend,
                completed,
                &audio,
                self.config,
                snapshot.pce(),
                snapshot.ws(),
            )?)
        } else {
            None
        };
        self.snapshots.insert(completed, snapshot);
        self.outputs.insert(
            completed,
            FrameBucket {
                ports: input.ports,
                audio,
                checkpoint,
            },
        );
        Ok(())
    }

    pub(super) fn correct(&mut self, backend: &mut EmuBackend) -> Result<u64> {
        let plan = self.timeline.correction()?;
        let Some(first) = plan.first() else {
            return Ok(0);
        };
        let frame = first.frame;
        let snapshot = self
            .snapshots
            .get(&frame)
            .context("rollback snapshot retired")?;
        self.lease.restore(backend, snapshot)?;
        self.snapshots.retain(|key, _| *key <= frame);
        self.outputs.retain(|key, _| *key <= frame);
        for &input in &plan {
            self.execute(backend, input)?;
        }
        self.timeline.corrected(&plan)?;
        ensure!(
            backend.frame_count() == self.timeline.frame(),
            "corrected core frame differs"
        );
        Ok(plan.len() as u64)
    }

    pub(super) fn commit(&mut self) -> Result<()> {
        let confirmed = self.timeline.confirmed_frame();
        while self
            .outputs
            .first_key_value()
            .is_some_and(|(frame, _)| *frame <= confirmed)
        {
            let (frame, bucket) = self
                .outputs
                .pop_first()
                .context("missing confirmed frame")?;
            if frame % checks::HASH_INTERVAL == 0 {
                let checkpoint = bucket
                    .checkpoint
                    .as_ref()
                    .context("missing periodic checkpoint")?;
                self.hashes.local(checkpoint.clone())?;
                self.network.send(checkpoint.clone())?;
            }
            let response = if self.verification {
                Response::Frame {
                    checkpoint: bucket
                        .checkpoint
                        .context("missing verification checkpoint")?,
                    ports: bucket.ports,
                    audio: bucket.audio,
                }
            } else {
                Response::Audio {
                    frame,
                    audio: bucket.audio,
                }
            };
            self.queue(response)?;
            self.committed = frame;
        }
        self.snapshots.retain(|frame, _| *frame >= confirmed);
        ensure!(
            self.snapshots.len() <= PREDICTION_WINDOW as usize + 1,
            "rollback snapshot window exceeded"
        );
        ensure!(
            self.outputs.len() <= PREDICTION_WINDOW as usize,
            "rollback output window exceeded"
        );
        Ok(())
    }

    pub(super) fn queue(&mut self, response: Response) -> Result<()> {
        ensure!(
            self.responses.len() < RESPONSE_CAPACITY,
            "netplay response queue is full"
        );
        self.responses.push_back(response);
        Ok(())
    }

    pub(super) fn present(
        &mut self,
        step_complete: bool,
        changed: bool,
        rollback_frames: u64,
    ) -> Result<()> {
        self.queue(Response::Presented {
            frame: self.timeline.frame(),
            confirmed: self.committed,
            changed,
            step_complete,
            prediction_depth: self.timeline.prediction_depth(),
            rollback_frames,
            retained_bytes: self.retained_bytes(),
        })
    }

    pub(super) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.checkpoint.capacity()
            + self.restore_snapshot.retained_bytes()
            + self
                .snapshots
                .values()
                .map(|state| {
                    state
                        .retained_bytes()
                        .saturating_sub(state.shared_media_bytes())
                })
                .sum::<usize>()
            + self
                .outputs
                .values()
                .map(|bucket| {
                    std::mem::size_of::<FrameBucket>()
                        + bucket.audio.capacity() * std::mem::size_of::<f32>()
                })
                .sum::<usize>()
            + self.responses.capacity() * std::mem::size_of::<Response>()
            + self
                .responses
                .iter()
                .map(|response| match response {
                    Response::Frame { audio, .. } | Response::Audio { audio, .. } => {
                        audio.capacity() * std::mem::size_of::<f32>()
                    }
                    Response::Chat { text, .. } | Response::ChatError(text) => text.capacity(),
                    _ => 0,
                })
                .sum::<usize>()
    }
}
