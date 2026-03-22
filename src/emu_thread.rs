use std::sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, Sender},
};
use std::thread::{self, JoinHandle};

use crate::emulator::Emulator;
use crate::hardware::types::CPUState;

pub(crate) enum EmuCommand {
    StepFrames {
        frames: usize,
        host_tilt: (f32, f32),
    },
    Shutdown,
}

pub(crate) enum EmuResponse {
    FrameReady { frame: Vec<u8>, rumble: bool },
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
                    EmuCommand::StepFrames { frames, host_tilt } => {
                        if frames == 0 {
                            continue;
                        }

                        let mut emu = match emulator.lock() {
                            Ok(emu) => emu,
                            Err(_) => break,
                        };

                        emu.set_mbc7_host_tilt(host_tilt.0, host_tilt.1);

                        if !matches!(emu.cpu.running, CPUState::Suspended) {
                            for _ in 0..frames {
                                emu.step_frame();
                                if matches!(emu.cpu.running, CPUState::Suspended) {
                                    break;
                                }
                            }
                        }

                        let frame = emu.framebuffer().to_vec();
                        let rumble = emu.bus.cartridge.rumble_active();
                        if resp_tx
                            .send(EmuResponse::FrameReady { frame, rumble })
                            .is_err()
                        {
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

    pub(crate) fn send_step_frames(&self, frames: usize, host_tilt: (f32, f32)) {
        let _ = self
            .cmd_tx
            .send(EmuCommand::StepFrames { frames, host_tilt });
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
