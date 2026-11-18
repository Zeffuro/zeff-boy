use super::*;
use std::num::NonZeroUsize;
use zeff_netplay::rollback::FrameInput;

const SLICE: NonZeroUsize = NonZeroUsize::new(64).unwrap();
const DEFERRED_CAPACITY: usize = 64;

struct Step {
    buttons: u16,
    sampled: bool,
    finished: bool,
    advanced: bool,
}

enum Transaction {
    Normal(FrameInput),
    Correction { plan: Vec<FrameInput>, index: usize },
}

impl Transaction {
    fn input(&self) -> FrameInput {
        match self {
            Self::Normal(input) => *input,
            Self::Correction { plan, index } => plan[*index],
        }
    }
}

#[derive(Default)]
pub(super) struct MdWork {
    step: Option<Step>,
    transaction: Option<Transaction>,
    deferred: VecDeque<Message>,
    changed: bool,
    rollback: u64,
}

impl MdWork {
    pub(super) fn payload_bytes(&self) -> Result<usize> {
        let plan = match &self.transaction {
            Some(Transaction::Correction { plan, .. }) => plan.capacity(),
            _ => 0,
        };
        self.deferred
            .capacity()
            .checked_mul(std::mem::size_of::<Message>())
            .and_then(|bytes| {
                plan.checked_mul(std::mem::size_of::<FrameInput>())
                    .and_then(|plan| bytes.checked_add(plan))
            })
            .context("MD pending work payload overflow")
    }
}

impl Session {
    pub(super) fn step_md(&mut self, buttons: u16) -> Result<()> {
        self.lease.validate_input(self.player, buttons)?;
        self.check_waiting()?;
        self.refresh_md_wait();
        ensure!(self.ready, "netplay admission is pending");
        let work = self.md_work.as_mut().context("MD work unavailable")?;
        ensure!(work.step.is_none(), "MD netplay step is already pending");
        work.step = Some(Step {
            buttons,
            sampled: false,
            finished: false,
            advanced: false,
        });
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn has_pending_work(&self) -> bool {
        self.md_work.as_ref().is_some_and(|work| {
            work.step.is_some() || work.transaction.is_some() || !work.deferred.is_empty()
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn accept_md_event(&mut self, event: Event) -> Result<Option<Response>> {
        self.check_waiting()?;
        self.receive_md_event(event)?;
        self.refresh_md_wait();
        Ok(self.responses.pop_front())
    }

    fn receive_md_event(&mut self, event: Event) -> Result<()> {
        let work = self.md_work.as_mut().context("MD work unavailable")?;
        if (work.transaction.is_some() || !work.deferred.is_empty())
            && let Event::Message(
                message @ (Message::Input { .. }
                | Message::PauseChange { .. }
                | Message::PauseAck { .. }
                | Message::Checkpoint { .. }
                | Message::Pause { .. }),
            ) = event
        {
            ensure!(
                work.deferred.len() < DEFERRED_CAPACITY,
                "MD deferred event queue is full"
            );
            work.deferred.push_back(message);
            return Ok(());
        }
        self.handle_event(event)
    }

    fn collect_md_events(&mut self, remaining: &mut usize) -> Result<()> {
        while *remaining != 0 {
            let work = self.md_work.as_mut().context("MD work unavailable")?;
            if work.transaction.is_none()
                && let Some(message) = work.deferred.pop_front()
            {
                self.handle_event(Event::Message(message))?;
                *remaining -= 1;
                continue;
            }
            let Some(event) = self.network.poll()? else {
                break;
            };
            self.receive_md_event(event)?;
            *remaining -= 1;
        }
        Ok(())
    }

    fn refresh_md_wait(&mut self) {
        if self.timeline.prediction_depth() < PREDICTION_WINDOW
            || self
                .pause
                .barrier()
                .is_some_and(|frame| self.timeline.confirmed_frame() >= frame)
        {
            self.stalled_since = None;
        }
    }

    fn begin_md_input(&mut self, backend: &mut EmuBackend, input: FrameInput) -> Result<()> {
        self.lease.validate_frame(backend, input.frame)?;
        self.lease.begin_md_frame(backend, input.ports)
    }

    fn prepare_md_transaction(&mut self, backend: &mut EmuBackend) -> Result<()> {
        let work = self.md_work.as_ref().context("MD work unavailable")?;
        if work.transaction.is_some() || !work.deferred.is_empty() || !self.ready {
            return Ok(());
        }
        let plan = self.timeline.correction()?;
        if let Some(input) = plan.first().copied() {
            let snapshot = self
                .snapshots
                .get(&input.frame)
                .context("rollback snapshot retired")?;
            self.lease.restore(backend, snapshot)?;
            self.snapshots.retain(|frame, _| *frame <= input.frame);
            self.outputs.retain(|frame, _| *frame <= input.frame);
            self.begin_md_input(backend, input)?;
            self.md_work.as_mut().unwrap().transaction =
                Some(Transaction::Correction { plan, index: 0 });
            return Ok(());
        }
        let Some(step) = self.md_work.as_ref().unwrap().step.as_ref() else {
            return Ok(());
        };
        if step.finished {
            return Ok(());
        }
        let frame = self.timeline.frame();
        if self.pause.barrier().is_some_and(|barrier| frame >= barrier) {
            self.md_work
                .as_mut()
                .unwrap()
                .step
                .as_mut()
                .unwrap()
                .finished = true;
            return Ok(());
        }
        if !step.sampled {
            let buttons = step.buttons;
            let (scheduled, value) = self.timeline.sample_local(buttons)?;
            if self.sampled != Some(scheduled) {
                self.network.send(Message::Input {
                    player: self.player,
                    frame: scheduled,
                    buttons: value,
                })?;
                self.sampled = Some(scheduled);
            }
            self.md_work
                .as_mut()
                .unwrap()
                .step
                .as_mut()
                .unwrap()
                .sampled = true;
        }
        if let Some(input) = self.timeline.next_frame()? {
            self.begin_md_input(backend, input)?;
            self.md_work.as_mut().unwrap().transaction = Some(Transaction::Normal(input));
        } else {
            self.md_work
                .as_mut()
                .unwrap()
                .step
                .as_mut()
                .unwrap()
                .finished = true;
        }
        Ok(())
    }

    fn complete_md_input(&mut self, backend: &mut EmuBackend, audio: Vec<f32>) -> Result<()> {
        let input = self
            .md_work
            .as_ref()
            .and_then(|work| work.transaction.as_ref())
            .context("MD field has no transaction")?
            .input();
        self.record_frame(
            backend,
            input,
            audio,
            #[cfg(all(test, not(target_arch = "wasm32")))]
            None,
        )?;
        let work = self.md_work.as_mut().unwrap();
        work.changed = true;
        let transaction = work
            .transaction
            .take()
            .context("MD field transaction disappeared")?;
        match transaction {
            Transaction::Normal(input) => {
                self.timeline.advance(input)?;
                let step = work
                    .step
                    .as_mut()
                    .context("MD executed field has no step")?;
                step.finished = true;
                step.advanced = true;
            }
            Transaction::Correction { plan, index } => {
                let next = index + 1;
                if next == plan.len() {
                    self.timeline.corrected(&plan)?;
                    self.lease.validate_frame(backend, self.timeline.frame())?;
                    work.rollback = work
                        .rollback
                        .checked_add(plan.len() as u64)
                        .context("rollback count overflow")?;
                } else {
                    let input = plan[next];
                    work.transaction = Some(Transaction::Correction { plan, index: next });
                    self.begin_md_input(backend, input)?;
                }
            }
        }
        Ok(())
    }

    fn finish_md_work(&mut self, backend: &EmuBackend) -> Result<()> {
        let work = self.md_work.as_ref().context("MD work unavailable")?;
        if work.transaction.is_some() || !work.deferred.is_empty() {
            return Ok(());
        }
        let committed_before = self.committed;
        self.commit()?;
        self.report_pause()?;
        let work = self.md_work.as_mut().unwrap();
        let completed = work.step.as_ref().is_some_and(|step| step.finished);
        if !completed && !work.changed && self.committed == committed_before {
            return Ok(());
        }
        if completed {
            let step = work.step.take().unwrap();
            let blocked = self
                .pause
                .barrier()
                .is_some_and(|barrier| self.timeline.frame() >= barrier);
            if step.advanced
                || (blocked && self.timeline.confirmed_frame() >= self.timeline.frame())
            {
                self.stalled_since = None;
            } else {
                self.stalled_since.get_or_insert_with(Instant::now);
            }
        }
        let changed = std::mem::take(&mut work.changed);
        let rollback = std::mem::take(&mut work.rollback);
        self.send_progress(completed)?;
        self.present(backend, completed, changed || rollback != 0, rollback)
    }

    pub(super) fn poll_md(&mut self, backend: &mut EmuBackend) -> Result<Option<Response>> {
        self.check_waiting()?;
        self.refresh_md_wait();
        if let Some(response) = self.responses.pop_front() {
            return Ok(Some(response));
        }
        let mut remaining = EVENT_BUDGET;
        self.collect_md_events(&mut remaining)?;
        self.refresh_md_wait();
        self.prepare_md_transaction(backend)?;
        if self.md_work.as_ref().unwrap().transaction.is_some()
            && let Some(audio) = self.lease.resume_md_frame(backend, SLICE)?
        {
            self.complete_md_input(backend, audio)?;
        }
        if self.md_work.as_ref().unwrap().transaction.is_none() {
            self.collect_md_events(&mut remaining)?;
            self.refresh_md_wait();
            self.prepare_md_transaction(backend)?;
        }
        self.finish_md_work(backend)?;
        self.send_progress(false)?;
        self.report_network_stats(Instant::now());
        self.check_waiting()?;
        Ok(self.responses.pop_front())
    }
}

#[cfg(test)]
mod tests;
