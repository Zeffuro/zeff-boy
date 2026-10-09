use super::*;
use zeff_netplay::wire::Message;

#[derive(Default)]
pub(super) struct Observation {
    pub(super) ready: bool,
    pub(super) pending: bool,
    pub(super) presented: u64,
    pub(super) replayed: u64,
    pub(super) frames: Vec<(Message, [u16; 2], Vec<f32>)>,
    pub(super) stopped: bool,
    pub(super) stop_reason: Option<String>,
}

impl Observation {
    pub(super) fn drain(&mut self, thread: &EmuThread, max_frames: usize) -> Result<()> {
        while thread.try_recv_frame().is_some() {}
        while let Some(response) = thread.try_recv_response() {
            match response {
                EmuResponse::Netplay(Response::Ready) => {
                    ensure!(!self.ready, "duplicate browser readiness");
                    self.ready = true;
                }
                EmuResponse::Netplay(Response::Presented {
                    frame,
                    step_complete,
                    rollback_frames,
                    ..
                }) => {
                    self.presented = frame;
                    self.replayed += rollback_frames;
                    if step_complete {
                        self.pending = false;
                    }
                }
                EmuResponse::Netplay(Response::Frame {
                    checkpoint,
                    ports,
                    audio,
                }) => {
                    ensure!(
                        self.frames.len() < max_frames && audio.len() <= 4096,
                        "browser ledger overflow"
                    );
                    ensure!(
                        audio.iter().all(|sample| sample.is_finite()),
                        "nonfinite browser PCM"
                    );
                    let Message::Checkpoint { frame, .. } = checkpoint else {
                        anyhow::bail!("browser frame lacks checkpoint");
                    };
                    ensure!(
                        frame == self.frames.len() as u64 + 1,
                        "browser confirmation gap or duplicate"
                    );
                    self.frames.push((checkpoint, ports, audio));
                }
                EmuResponse::Netplay(Response::NetworkStats(_)) => {}
                EmuResponse::Netplay(Response::Stopped { reason, restored }) => {
                    ensure!(
                        restored && !self.stopped,
                        "browser restoration failed or duplicate stop: {reason}"
                    );
                    self.stop_reason = Some(reason);
                    self.stopped = true;
                }
                EmuResponse::Netplay(Response::Rejected(reason)) => {
                    anyhow::bail!("browser rejection: {reason}")
                }
                _ => anyhow::bail!("unexpected browser gameplay response"),
            }
        }
        Ok(())
    }
}
