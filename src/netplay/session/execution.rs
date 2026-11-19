use super::*;
use zeff_netplay::rollback::FrameInput;

pub(super) struct FrameBucket {
    ports: [u16; 2],
    audio: Vec<f32>,
    checkpoint: Option<Message>,
}

impl Session {
    pub(super) fn execute(&mut self, backend: &mut EmuBackend, input: FrameInput) -> Result<()> {
        self.lease.validate_frame(backend, input.frame)?;
        #[cfg(all(test, not(target_arch = "wasm32")))]
        let mut timing = profile::FrameTimer::begin(self.profile.as_ref())?;
        let audio = self.lease.advance(backend, input.ports)?;
        #[cfg(all(test, not(target_arch = "wasm32")))]
        if let Some(timer) = timing.as_mut() {
            timer.advanced();
        }
        self.record_frame(
            backend,
            input,
            audio,
            #[cfg(all(test, not(target_arch = "wasm32")))]
            timing,
        )
    }

    pub(super) fn record_frame(
        &mut self,
        backend: &EmuBackend,
        input: FrameInput,
        audio: Vec<f32>,
        #[cfg(all(test, not(target_arch = "wasm32")))] mut timing: Option<profile::FrameTimer>,
    ) -> Result<()> {
        let completed = input.frame.checked_add(1).context("frame overflow")?;
        let snapshot = self.lease.capture(backend)?;
        ensure!(
            snapshot.frame() == completed,
            "rollback snapshot frame differs"
        );
        #[cfg(all(test, not(target_arch = "wasm32")))]
        if let Some(timer) = timing.as_mut() {
            timer.captured();
        }
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
        #[cfg(all(test, not(target_arch = "wasm32")))]
        let checkpoint_cost = timing
            .as_ref()
            .and_then(|timer| timer.checkpointed(checkpoint.is_some()));
        self.snapshots.insert(completed, snapshot);
        self.outputs.insert(
            completed,
            FrameBucket {
                ports: input.ports,
                audio,
                checkpoint,
            },
        );
        #[cfg(all(test, not(target_arch = "wasm32")))]
        if let (Some(profile), Some(timer)) = (self.profile.as_mut(), timing) {
            profile
                .frames
                .push(timer.finish(completed, checkpoint_cost));
        }
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
        self.lease.validate_frame(backend, self.timeline.frame())?;
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
        backend: &EmuBackend,
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
            retained_bytes: self.retained_bytes_with_backend(backend)?,
        })
    }

    fn retained_bytes_with_backend(&self, backend: &EmuBackend) -> Result<usize> {
        if !self.lease.is_md() {
            return self.retained_bytes();
        }
        let EmuBackend::Md(md) = backend else {
            anyhow::bail!("netplay lost MD backend");
        };
        let mut into = zeff_md_core::emulator::rollback::MdRetainedAllocations::new();
        md.visit_rollback_core_allocations(&mut into)?;
        let Snapshot::Md {
            state: original, ..
        } = &self.restore_snapshot
        else {
            anyhow::bail!("MD baseline snapshot differs");
        };
        original.visit_retained_allocations(&mut into)?;
        for snapshot in self.snapshots.values() {
            let Snapshot::Md {
                state: snapshot, ..
            } = snapshot
            else {
                anyhow::bail!("MD rollback snapshot differs");
            };
            snapshot.visit_retained_allocations(&mut into)?;
        }
        let core = usize::try_from(into.finish()?.requested_payload_bytes())?;
        core.checked_add(self.retained_host_payload()?)
            .and_then(|bytes| bytes.checked_add(md.netplay_load_payload_bytes()))
            .context("MD retained payload overflow")
    }

    pub(super) fn retained_bytes(&self) -> Result<usize> {
        let mut bytes = self.retained_host_payload()?;
        bytes = bytes
            .checked_add(self.restore_snapshot.retained_bytes()?)
            .context("retained payload overflow")?;
        for state in self.snapshots.values() {
            let state = state
                .retained_bytes()?
                .saturating_sub(state.shared_media_bytes());
            bytes = bytes
                .checked_add(state)
                .context("retained payload overflow")?;
        }
        Ok(bytes)
    }

    fn retained_host_payload(&self) -> Result<usize> {
        let mut bytes = std::mem::size_of::<Self>();
        let mut add = |amount: usize| -> Result<()> {
            bytes = bytes
                .checked_add(amount)
                .context("retained payload overflow")?;
            Ok(())
        };
        add(self.checkpoint.capacity())?;
        if let Some(work) = &self.md_work {
            add(work.payload_bytes()?)?;
        }
        if self.lease.is_md() {
            let wrapper = std::mem::size_of::<crate::emu_backend::md::MdBackendRollbackSnapshot>()
                .checked_sub(std::mem::size_of::<
                    zeff_md_core::emulator::rollback::MdRollbackSnapshot,
                >())
                .context("MD snapshot wrapper layout differs")?;
            add(wrapper)?;
            for _ in self.snapshots.values() {
                add(std::mem::size_of::<Snapshot>())?;
                add(wrapper)?;
            }
        }
        for bucket in self.outputs.values() {
            add(std::mem::size_of::<FrameBucket>())?;
            add(bucket
                .audio
                .capacity()
                .checked_mul(std::mem::size_of::<f32>())
                .context("audio capacity overflow")?)?;
        }
        add(self
            .responses
            .capacity()
            .checked_mul(std::mem::size_of::<Response>())
            .context("response capacity overflow")?)?;
        for response in &self.responses {
            match response {
                Response::Frame { audio, .. } | Response::Audio { audio, .. } => {
                    add(audio
                        .capacity()
                        .checked_mul(std::mem::size_of::<f32>())
                        .context("audio capacity overflow")?)?;
                }
                Response::Chat { text, .. } | Response::ChatError(text) => add(text.capacity())?,
                _ => {}
            }
        }
        Ok(bytes)
    }
}
