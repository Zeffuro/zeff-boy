use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{self as chan, Receiver, Sender};

use super::emu_loop;
use super::types::{self, EmuCommand, EmuResponse, FrameResult, SharedFramebuffer};
use crate::emu_backend::{CoreCapabilities, EmuBackend};
use zeff_emu_common::time::MachineTiming;

const FRAME_CHANNEL_CAPACITY: usize = 2;
const SHUTDOWN_TIMEOUT_SECS: u64 = 5;

pub(crate) enum EmuResponsePoll {
    Response(EmuResponse),
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

impl EmuThread {
    pub(crate) fn spawn(backend: EmuBackend, save_recovery_on_shutdown: bool) -> Self {
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

        let join = thread::spawn(move || {
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
                    recovery: None,
                },
            );
            emu_loop.set_frame_duration_handle(emu_frame_duration_ns);
            emu_loop.run();
        });

        Self {
            cmd_tx,
            frame_rx,
            resp_rx,
            join: Some(join),
            shared_framebuffer: shared_fb,
            capabilities,
            frame_duration_ns: shared_frame_duration_ns,
            audio_recording_context,
        }
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
            Ok(response) => EmuResponsePoll::Response(response),
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
}

impl Drop for EmuThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests;
