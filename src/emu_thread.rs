use std::sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, Sender},
};
use std::thread::{self, JoinHandle};

use crate::emulator::Emulator;
use crate::hardware::types::CPUState;

pub(crate) enum EmuCommand {
    StepFrames(usize),
    Shutdown,
}

pub(crate) enum EmuResponse {
    FrameReady(Vec<u8>),
    AudioSamples(Vec<f32>),
}

pub(crate) struct EmuThread {
    cmd_tx: Sender<EmuCommand>,
    resp_rx: Receiver<EmuResponse>,
    join: Option<JoinHandle<()>>,
}

impl EmuThread {
    pub(crate) fn spawn(emulator: Arc<Mutex<Emulator>>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (resp_tx, resp_rx) = mpsc::channel();

        let join = thread::spawn(move || {
            while let Ok(command) = cmd_rx.recv() {
                match command {
                    EmuCommand::StepFrames(frames) => {
                        if frames == 0 {
                            continue;
                        }

                        let mut emu = match emulator.lock() {
                            Ok(emu) => emu,
                            Err(_) => break,
                        };

                        if !matches!(emu.cpu.running, CPUState::Suspended) {
                            for _ in 0..frames {
                                emu.step_frame();
                                if matches!(emu.cpu.running, CPUState::Suspended) {
                                    break;
                                }
                            }
                        }

                        let frame = emu.framebuffer().to_vec();
                        if resp_tx.send(EmuResponse::FrameReady(frame)).is_err() {
                            break;
                        }

                        let samples = emu.bus.io.apu.drain_samples();
                        if !samples.is_empty()
                            && resp_tx.send(EmuResponse::AudioSamples(samples)).is_err()
                        {
                            break;
                        }
                    }
                    EmuCommand::Shutdown => break,
                }
            }
        });

        Self {
            cmd_tx,
            resp_rx,
            join: Some(join),
        }
    }

    pub(crate) fn send_step_frames(&self, frames: usize) {
        let _ = self.cmd_tx.send(EmuCommand::StepFrames(frames));
    }

    pub(crate) fn try_recv(&self) -> Option<EmuResponse> {
        self.resp_rx.try_recv().ok()
    }

    pub(crate) fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(EmuCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for EmuThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}
