use super::*;

#[derive(Default)]
struct Vote {
    target: Option<u64>,
    paused: bool,
}

pub(super) struct PauseControl {
    lead: u64,
    local: Vote,
    peer: Vote,
    episode: Option<u64>,
    reported: Option<(bool, bool)>,
    local_request: u64,
    local_acked: u64,
    peer_request: u64,
    desired: bool,
}

impl Default for PauseControl {
    fn default() -> Self {
        Self::with_delay(zeff_netplay::rollback::InputDelay::default())
    }
}

impl PauseControl {
    pub(super) fn with_delay(delay: zeff_netplay::rollback::InputDelay) -> Self {
        Self {
            lead: delay.pause_lead(),
            local: Vote::default(),
            peer: Vote::default(),
            episode: None,
            reported: None,
            local_request: 0,
            local_acked: 0,
            peer_request: 0,
            desired: false,
        }
    }
    pub(super) fn request(&mut self, current: u64, paused: bool) -> Result<Option<Message>> {
        self.desired = paused;
        self.reconcile(current)
    }

    fn reconcile(&mut self, current: u64) -> Result<Option<Message>> {
        let paused = self.desired;
        if self.local.paused == paused || (paused && self.local_acked != self.local_request) {
            return Ok(None);
        }
        let target = if paused {
            self.barrier()
                .filter(|target| *target >= current)
                .unwrap_or(
                    current
                        .checked_add(self.lead)
                        .context("pause frame overflow")?,
                )
        } else {
            self.local.target.context("missing local pause target")?
        };
        let request = self
            .local_request
            .checked_add(1)
            .context("pause request ID exhausted")?;
        self.local = Vote {
            target: Some(target),
            paused,
        };
        self.local_request = request;
        self.update_episode(target, paused);
        Ok(Some(Message::PauseChange {
            request,
            frame: target,
            paused,
        }))
    }

    pub(super) fn observe(
        &mut self,
        current: u64,
        request: u64,
        target: u64,
        paused: bool,
    ) -> Result<Message> {
        ensure!(request != 0, "invalid peer pause request ID");
        if self.peer_request == request {
            ensure!(
                self.peer.target == Some(target) && self.peer.paused == paused,
                "peer rewrote a pause request"
            );
            return Ok(Message::PauseAck { request });
        }
        ensure!(
            request
                == self
                    .peer_request
                    .checked_add(1)
                    .context("peer pause request ID exhausted")?,
            "out-of-order peer pause request"
        );
        ensure!(
            self.peer.paused != paused,
            "peer pause request did not change state"
        );
        if paused {
            ensure!(!self.peer.paused, "peer changed an active pause target");
            ensure!(
                target >= current
                    && target
                        <= current
                            .checked_add(2 * self.lead)
                            .context("pause frame overflow")?,
                "peer pause target exceeds window"
            );
        } else {
            ensure!(
                self.peer.target == Some(target),
                "peer released a different pause target"
            );
        }
        self.peer = Vote {
            target: Some(target),
            paused,
        };
        self.peer_request = request;
        self.update_episode(target, paused);
        Ok(Message::PauseAck { request })
    }

    pub(super) fn acknowledge(&mut self, current: u64, request: u64) -> Result<Option<Message>> {
        ensure!(
            request != 0 && request <= self.local_request,
            "unknown pause acknowledgment"
        );
        self.local_acked = self.local_acked.max(request);
        self.finish_episode();
        self.reconcile(current)
    }

    pub(super) fn barrier(&self) -> Option<u64> {
        self.episode
    }

    fn update_episode(&mut self, target: u64, paused: bool) {
        if paused {
            self.episode = Some(self.episode.map_or(target, |barrier| barrier.min(target)));
        } else {
            self.finish_episode();
        }
    }

    fn finish_episode(&mut self) {
        if !self.local.paused && !self.peer.paused && self.local_acked == self.local_request {
            self.episode = None;
        }
    }

    pub(super) fn status(&mut self, frame: u64, confirmed: u64) -> Option<(bool, bool)> {
        let flags = (self.local.paused, self.peer.paused);
        if let Some(barrier) = self.barrier() {
            if frame < barrier {
                return self.reported.take().map(|_| (false, false));
            }
            if confirmed < barrier {
                return None;
            }
            if flags == (false, false) {
                return None;
            }
        } else if self.reported.is_none() {
            return None;
        }
        if self.reported == Some(flags) {
            return None;
        }
        self.reported = (flags != (false, false)).then_some(flags);
        Some(flags)
    }
}
