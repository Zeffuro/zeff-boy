use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{self as chan, Receiver, Sender};

use super::emu_loop;
use super::types::{
    self, EmuCommand, EmuResponse, FrameResult, SharedFramebuffer,
    TasPersistencePublicationOutcome, TasRepairAction, TasRepairActionRejectedReason,
    TasRepairIdentity, TasRepairSuspendRejectedReason, TasRepairSuspensionProof,
};
use crate::emu_backend::{CoreCapabilities, EmuBackend};
use zeff_emu_common::time::MachineTiming;

const FRAME_CHANNEL_CAPACITY: usize = 2;
const SHUTDOWN_TIMEOUT_SECS: u64 = 5;
const TAS_REPAIR_TIMEOUT_SECS: u64 = 5;

pub(crate) enum EmuResponsePoll {
    Response(Box<EmuResponse>),
    Empty,
    Disconnected,
}

pub(crate) struct EmuThread {
    cmd_tx: Sender<EmuCommand>,
    frame_rx: Receiver<FrameResult>,
    resp_rx: Receiver<EmuResponse>,
    join: Option<JoinHandle<()>>,
    shared_framebuffer: SharedFramebuffer,
    capabilities: CoreCapabilities,
    frame_duration_ns: Arc<AtomicU64>,
    audio_recording_context: Option<crate::audio_tooling::AudioRecordingContext>,
}

pub(crate) struct SuspendedEmuThread {
    worker: Option<EmuThread>,
    proof: TasRepairSuspensionProof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasRepairSuspendFailure {
    ChannelClosed,
    TimedOut,
    Rejected(TasRepairSuspendRejectedReason),
    UnexpectedResponse,
}

pub(crate) struct TasRepairSuspendError {
    pub(crate) reason: TasRepairSuspendFailure,
    pub(crate) original_worker: Option<Box<EmuThread>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasRepairReleaseFailure {
    ChannelClosed,
    TimedOut,
    Rejected(TasRepairActionRejectedReason),
    UnexpectedResponse,
}

impl EmuThread {
    pub(crate) fn spawn(backend: EmuBackend, save_recovery_on_shutdown: bool) -> Self {
        Self::try_spawn_inner(
            backend,
            save_recovery_on_shutdown,
            None,
            #[cfg(test)]
            None,
        )
        .expect("failed to spawn emulator thread")
    }

    pub(crate) fn try_spawn_repaired(
        backend: EmuBackend,
        identity: TasRepairIdentity,
    ) -> std::io::Result<Self> {
        Self::try_spawn_inner(
            backend,
            false,
            Some(identity),
            #[cfg(test)]
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn try_spawn_repaired_with_recovery(
        backend: EmuBackend,
        identity: TasRepairIdentity,
        recovery: super::RecoveryTestConfig,
    ) -> std::io::Result<Self> {
        Self::try_spawn_inner(backend, false, Some(identity), Some(recovery))
    }

    fn try_spawn_inner(
        backend: EmuBackend,
        save_recovery_on_shutdown: bool,
        repair_identity: Option<TasRepairIdentity>,
        #[cfg(test)] recovery: Option<super::RecoveryTestConfig>,
    ) -> std::io::Result<Self> {
        let capabilities = backend.capabilities();
        let frame_duration_ns = backend.nominal_frame_duration_ns();
        let shared_frame_duration_ns = Arc::new(AtomicU64::new(frame_duration_ns));
        let audio_recording_context =
            backend
                .audio_topology()
                .map(|topology| crate::audio_tooling::AudioRecordingContext {
                    system: backend.system(),
                    topology,
                    clock_rate: backend.timing_snapshot().rate(),
                });
        let (cmd_tx, cmd_rx) = chan::unbounded();
        let (frame_tx, frame_rx) = chan::bounded::<FrameResult>(FRAME_CHANNEL_CAPACITY);
        let (resp_tx, resp_rx) = chan::unbounded();

        let drain_rx = frame_rx.clone();

        let shared_fb = types::new_shared_framebuffer();
        types::publish_framebuffer(&shared_fb, backend.framebuffer());
        let emu_fb = shared_fb.clone();
        let emu_frame_duration_ns = Arc::clone(&shared_frame_duration_ns);

        let join = thread::Builder::new()
            .name("zeff-emu".to_owned())
            .spawn(move || {
                let mut emu_loop = emu_loop::EmuLoop::new(
                    backend,
                    cmd_rx,
                    frame_tx,
                    drain_rx,
                    resp_tx,
                    emu_loop::EmuLoopConfig {
                        shared_framebuffer: emu_fb,
                        save_recovery_on_shutdown,
                        #[cfg(test)]
                        recovery,
                    },
                );
                emu_loop.set_frame_duration_handle(emu_frame_duration_ns);
                if let Some(identity) = repair_identity {
                    emu_loop.set_tas_repair_identity(identity);
                }
                emu_loop.run();
            })?;

        Ok(Self {
            cmd_tx,
            frame_rx,
            resp_rx,
            join: Some(join),
            shared_framebuffer: shared_fb,
            capabilities,
            frame_duration_ns: shared_frame_duration_ns,
            audio_recording_context,
        })
    }

    pub(crate) fn audio_recording_context(
        &self,
    ) -> Option<crate::audio_tooling::AudioRecordingContext> {
        self.audio_recording_context
    }

    pub(crate) fn capabilities(&self) -> &CoreCapabilities {
        &self.capabilities
    }

    pub(crate) fn nominal_frame_duration_ns(&self) -> u64 {
        self.frame_duration_ns.load(Ordering::Acquire)
    }

    pub(crate) fn shared_framebuffer(&self) -> &SharedFramebuffer {
        &self.shared_framebuffer
    }

    pub(crate) fn send(&self, cmd: EmuCommand) {
        self.send_checked(cmd);
    }

    pub(crate) fn send_checked(&self, cmd: EmuCommand) -> bool {
        if self.cmd_tx.send(cmd).is_err() {
            log::warn!("Failed to send command to emu thread (channel closed)");
            false
        } else {
            true
        }
    }

    pub(crate) fn suspend_for_tas_repair(
        mut self,
        identity: TasRepairIdentity,
    ) -> Result<SuspendedEmuThread, TasRepairSuspendError> {
        if self
            .cmd_tx
            .send(EmuCommand::SuspendTasRepair { identity })
            .is_err()
        {
            self.join_terminated();
            return Err(TasRepairSuspendError {
                reason: TasRepairSuspendFailure::ChannelClosed,
                original_worker: None,
            });
        }
        let response = match self
            .resp_rx
            .recv_timeout(Duration::from_secs(TAS_REPAIR_TIMEOUT_SECS))
        {
            Ok(response) => response,
            Err(chan::RecvTimeoutError::Timeout) => {
                self.discard_uncertain_suspension(identity);
                return Err(TasRepairSuspendError {
                    reason: TasRepairSuspendFailure::TimedOut,
                    original_worker: None,
                });
            }
            Err(chan::RecvTimeoutError::Disconnected) => {
                self.join_terminated();
                return Err(TasRepairSuspendError {
                    reason: TasRepairSuspendFailure::ChannelClosed,
                    original_worker: None,
                });
            }
        };
        match response {
            EmuResponse::TasRepairSuspended { proof } if proof.identity == identity => {
                Ok(SuspendedEmuThread {
                    worker: Some(self),
                    proof: *proof,
                })
            }
            EmuResponse::TasRepairSuspendRejected {
                identity: rejected,
                reason,
            } if rejected == identity => Err(TasRepairSuspendError {
                reason: TasRepairSuspendFailure::Rejected(reason),
                original_worker: Some(Box::new(self)),
            }),
            _ => {
                self.discard_uncertain_suspension(identity);
                Err(TasRepairSuspendError {
                    reason: TasRepairSuspendFailure::UnexpectedResponse,
                    original_worker: None,
                })
            }
        }
    }

    pub(crate) fn discard_repaired_for_tas_restore(
        mut self,
        identity: TasRepairIdentity,
    ) -> Result<(), TasRepairReleaseFailure> {
        if self
            .cmd_tx
            .send(EmuCommand::DiscardRepairedTasWorker { identity })
            .is_err()
        {
            self.join_terminated();
            return Err(TasRepairReleaseFailure::ChannelClosed);
        }
        let result = match self
            .resp_rx
            .recv_timeout(Duration::from_secs(TAS_REPAIR_TIMEOUT_SECS))
        {
            Ok(EmuResponse::TasRepairRepairedWorkerDiscarded {
                identity: discarded,
            }) if discarded == identity => Ok(()),
            Ok(EmuResponse::TasRepairActionRejected {
                identity: rejected,
                action: TasRepairAction::DiscardRepaired,
                reason,
            }) if rejected == identity => Err(TasRepairReleaseFailure::Rejected(reason)),
            Ok(_) => Err(TasRepairReleaseFailure::UnexpectedResponse),
            Err(chan::RecvTimeoutError::Timeout) => Err(TasRepairReleaseFailure::TimedOut),
            Err(chan::RecvTimeoutError::Disconnected) => {
                Err(TasRepairReleaseFailure::ChannelClosed)
            }
        };
        self.join_terminated();
        result
    }

    pub(crate) fn commit_repaired_tas_worker(
        &self,
        identity: TasRepairIdentity,
        save_recovery_on_shutdown: bool,
    ) -> Result<TasPersistencePublicationOutcome, TasRepairReleaseFailure> {
        if self
            .cmd_tx
            .send(EmuCommand::CommitRepairedTasWorker {
                identity,
                save_recovery_on_shutdown,
            })
            .is_err()
        {
            return Err(TasRepairReleaseFailure::ChannelClosed);
        }
        match self
            .resp_rx
            .recv_timeout(Duration::from_secs(TAS_REPAIR_TIMEOUT_SECS))
        {
            Ok(EmuResponse::TasRepairRepairedWorkerCommitted {
                identity: committed,
                publication,
            }) if *committed == identity => Ok(publication),
            Ok(EmuResponse::TasRepairActionRejected {
                identity: rejected,
                action: TasRepairAction::CommitRepaired,
                reason,
            }) if rejected == identity => Err(TasRepairReleaseFailure::Rejected(reason)),
            Ok(_) => Ok(
                TasPersistencePublicationOutcome::PublishedDurabilityUncertain {
                    path: None,
                    error: "unexpected repaired-worker response after Keep was sent".to_owned(),
                },
            ),
            Err(chan::RecvTimeoutError::Timeout) => Ok(
                TasPersistencePublicationOutcome::PublishedDurabilityUncertain {
                    path: None,
                    error: "timed out after repaired-worker Keep was sent".to_owned(),
                },
            ),
            Err(chan::RecvTimeoutError::Disconnected) => Ok(
                TasPersistencePublicationOutcome::PublishedDurabilityUncertain {
                    path: None,
                    error: "repaired worker disconnected after Keep was sent".to_owned(),
                },
            ),
        }
    }

    pub(crate) fn try_recv_frame(&self) -> Option<FrameResult> {
        self.frame_rx.try_recv().ok()
    }

    pub(crate) fn recv(&self) -> Option<EmuResponse> {
        self.resp_rx.recv().ok()
    }

    pub(crate) fn recv_checked(&self) -> Result<EmuResponse, ()> {
        self.resp_rx.recv().map_err(|_| ())
    }

    pub(crate) fn poll_response(&self) -> EmuResponsePoll {
        match self.resp_rx.try_recv() {
            Ok(response) => EmuResponsePoll::Response(Box::new(response)),
            Err(chan::TryRecvError::Empty) => EmuResponsePoll::Empty,
            Err(chan::TryRecvError::Disconnected) => EmuResponsePoll::Disconnected,
        }
    }

    pub(crate) fn shutdown(&mut self) {
        if self.cmd_tx.send(EmuCommand::Shutdown).is_err() {
            log::debug!("Shutdown command could not be sent (channel closed)");
        }
        while self.frame_rx.try_recv().is_ok() {}

        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(SHUTDOWN_TIMEOUT_SECS);
        loop {
            let timeout = deadline.saturating_duration_since(std::time::Instant::now());
            if timeout.is_zero() {
                log::warn!("Emu thread shutdown timed out after {SHUTDOWN_TIMEOUT_SECS}s");
                break;
            }
            match self.resp_rx.recv_timeout(timeout) {
                Ok(EmuResponse::ShutdownComplete) => break,
                Ok(EmuResponse::SramFlushed(Some(path))) => {
                    log::info!("Saved battery RAM to {}", path);
                }
                Ok(EmuResponse::SramFlushFailed(error)) => {
                    log::warn!("Battery save failed: {error}");
                }
                Ok(EmuResponse::RecoverySaved(path)) => {
                    log::info!("Saved recovery state to {}", path.display());
                }
                Ok(EmuResponse::RecoverySaveFailed(error)) => {
                    log::warn!("Recovery save failed: {error}");
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            log::error!("emulator thread panicked during shutdown");
        }
    }

    fn join_terminated(&mut self) {
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            log::error!("emulator thread panicked during TAS repair transition");
        }
    }

    fn discard_uncertain_suspension(&mut self, identity: TasRepairIdentity) {
        let _ = self.cmd_tx.send(EmuCommand::DiscardTasRepair { identity });
        let _ = self
            .resp_rx
            .recv_timeout(Duration::from_secs(TAS_REPAIR_TIMEOUT_SECS));
        self.join_terminated();
    }
}

impl SuspendedEmuThread {
    pub(crate) fn proof(&self) -> &TasRepairSuspensionProof {
        &self.proof
    }

    pub(crate) fn resume(mut self) -> Result<EmuThread, TasRepairReleaseFailure> {
        let identity = self.proof.identity;
        let Some(worker) = self.worker.as_ref() else {
            return Err(TasRepairReleaseFailure::ChannelClosed);
        };
        if worker
            .cmd_tx
            .send(EmuCommand::ResumeTasRepair {
                identity,
                expected_proof: Box::new(self.proof.clone()),
            })
            .is_err()
        {
            return Err(TasRepairReleaseFailure::ChannelClosed);
        }
        match worker
            .resp_rx
            .recv_timeout(Duration::from_secs(TAS_REPAIR_TIMEOUT_SECS))
        {
            Ok(EmuResponse::TasRepairOriginalResumed { proof }) if *proof == self.proof => self
                .worker
                .take()
                .ok_or(TasRepairReleaseFailure::ChannelClosed),
            Ok(EmuResponse::TasRepairActionRejected {
                identity: rejected,
                action: TasRepairAction::ResumeOriginal,
                reason,
            }) if rejected == identity => Err(TasRepairReleaseFailure::Rejected(reason)),
            Ok(_) => Err(TasRepairReleaseFailure::UnexpectedResponse),
            Err(chan::RecvTimeoutError::Timeout) => Err(TasRepairReleaseFailure::TimedOut),
            Err(chan::RecvTimeoutError::Disconnected) => {
                Err(TasRepairReleaseFailure::ChannelClosed)
            }
        }
    }

    pub(crate) fn discard(mut self) -> Result<(), TasRepairReleaseFailure> {
        let result = self.discard_inner();
        if result.is_ok()
            && let Some(mut worker) = self.worker.take()
        {
            worker.join_terminated();
        }
        result
    }

    fn discard_inner(&mut self) -> Result<(), TasRepairReleaseFailure> {
        let identity = self.proof.identity;
        let Some(worker) = self.worker.as_ref() else {
            return Ok(());
        };
        if worker
            .cmd_tx
            .send(EmuCommand::DiscardTasRepair { identity })
            .is_err()
        {
            return Err(TasRepairReleaseFailure::ChannelClosed);
        }
        match worker
            .resp_rx
            .recv_timeout(Duration::from_secs(TAS_REPAIR_TIMEOUT_SECS))
        {
            Ok(EmuResponse::TasRepairOriginalDiscarded {
                identity: discarded,
            }) if discarded == identity => Ok(()),
            Ok(EmuResponse::TasRepairActionRejected {
                identity: rejected,
                action: TasRepairAction::DiscardOriginal,
                reason,
            }) if rejected == identity => Err(TasRepairReleaseFailure::Rejected(reason)),
            Ok(_) => Err(TasRepairReleaseFailure::UnexpectedResponse),
            Err(chan::RecvTimeoutError::Timeout) => Err(TasRepairReleaseFailure::TimedOut),
            Err(chan::RecvTimeoutError::Disconnected) => {
                Err(TasRepairReleaseFailure::ChannelClosed)
            }
        }
    }
}

impl Drop for SuspendedEmuThread {
    fn drop(&mut self) {
        if self.worker.is_none() {
            return;
        }
        let _ = self.discard_inner();
        if let Some(mut worker) = self.worker.take() {
            worker.join_terminated();
        }
    }
}

impl Drop for EmuThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests;
