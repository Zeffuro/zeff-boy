use std::thread::{self, JoinHandle};

use crossbeam_channel::{self as chan, Receiver, Sender};

use super::emu_loop;
use super::types::{self, EmuCommand, EmuResponse, FrameResult, SharedFramebuffer};
use crate::emu_backend::EmuBackend;

const FRAME_CHANNEL_CAPACITY: usize = 1;
const SHUTDOWN_TIMEOUT_SECS: u64 = 5;

pub(crate) struct EmuThread {
    cmd_tx: Sender<EmuCommand>,
    frame_rx: Receiver<FrameResult>,
    resp_rx: Receiver<EmuResponse>,
    join: Option<JoinHandle<()>>,
    shared_framebuffer: SharedFramebuffer,
}

impl EmuThread {
    pub(crate) fn spawn(backend: EmuBackend) -> Self {
        let (cmd_tx, cmd_rx) = chan::unbounded();
        let (frame_tx, frame_rx) = chan::bounded::<FrameResult>(FRAME_CHANNEL_CAPACITY);
        let (resp_tx, resp_rx) = chan::unbounded();

        let drain_rx = frame_rx.clone();

        let shared_fb = types::new_shared_framebuffer();
        let emu_fb = shared_fb.clone();

        let join = thread::spawn(move || {
            let mut emu_loop =
                emu_loop::EmuLoop::new(backend, cmd_rx, frame_tx, drain_rx, resp_tx, emu_fb);
            emu_loop.run();
        });

        Self {
            cmd_tx,
            frame_rx,
            resp_rx,
            join: Some(join),
            shared_framebuffer: shared_fb,
        }
    }

    pub(crate) fn shared_framebuffer(&self) -> &SharedFramebuffer {
        &self.shared_framebuffer
    }

    pub(crate) fn send(&self, cmd: EmuCommand) {
        if self.cmd_tx.send(cmd).is_err() {
            log::warn!("Failed to send command to emu thread (channel closed)");
        }
    }

    pub(crate) fn try_recv_frame(&self) -> Option<FrameResult> {
        self.frame_rx.try_recv().ok()
    }

    pub(crate) fn recv(&self) -> Option<EmuResponse> {
        self.resp_rx.recv().ok()
    }

    pub(crate) fn try_recv_response(&self) -> Option<EmuResponse> {
        self.resp_rx.try_recv().ok()
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
