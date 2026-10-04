use anyhow::{Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zeff_nes_core::emulator::Emulator;
use zeff_nes_core::save_state::{
    NES_SAVE_STATE_FORMAT_VERSION, TAS_DETERMINISM_ABI_ID, TAS_STATE_FORMAT_COMPATIBILITY_ID,
    project_replay_state_bytes,
};

use crate::lockstep::{ConfirmedInput, INPUT_DELAY, Player};
use crate::wire::{Identity, Message};

use crate::fixture;
mod session;
pub use session::{PeerSetup, Scenario, run_peer, run_peer_with_setup};

const CHECKPOINT_ABI: &[u8] = b"ZeffNetplay-NES-replay-v10-v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Checkpoint {
    pub frame: u64,
    pub logical: [u8; 32],
    pub video: [u8; 32],
    pub audio: [u8; 32],
    pub persistent: [u8; 32],
}

impl Checkpoint {
    fn message(&self) -> Message {
        Message::Checkpoint {
            frame: self.frame,
            logical: self.logical,
            video: self.video,
            audio: self.audio,
            persistent: self.persistent,
        }
    }
}

// Owning the core without filesystem/publication APIs leases all save mutations
// to this disposable experiment, including after admission failure/disconnect.
struct Machine {
    emu: Emulator,
    frame: u64,
    config: [u8; 32],
    closed: bool,
}

impl Machine {
    fn new() -> Result<Self> {
        let emu = Emulator::from_rom_data(&fixture::rom())?;
        ensure!(emu.has_standard_console_hardware());
        ensure!(emu.has_standard_controller_topology());
        ensure!(emu.has_battery());
        let mut config = Sha256::new();
        config.update(
            b"NES/NTSC/NINA001/P1-P2/48000/zero-SRAM/no-patches-firmware-cheats/lease-discard/v1",
        );
        config.update(INPUT_DELAY.to_le_bytes());
        config.update(TAS_DETERMINISM_ABI_ID.as_bytes());
        config.update(TAS_STATE_FORMAT_COMPATIBILITY_ID.as_bytes());
        Ok(Self {
            emu,
            frame: 0,
            config: config.finalize().into(),
            closed: false,
        })
    }

    fn identity(&self, build: [u8; 32]) -> Result<Identity> {
        let media = fixture::rom();
        let digest = Sha256::digest(&media).into();
        Ok(Identity {
            build,
            build_info: Default::default(),
            source: digest,
            effective: digest,
            media_len: media.len() as u64,
            config: self.config,
            initial: self.logical()?,
            persistent: self.persistent(),
            state_format: NES_SAVE_STATE_FORMAT_VERSION,
        })
    }

    fn logical(&self) -> Result<[u8; 32]> {
        let mut bytes = self.emu.encode_state()?;
        project_replay_state_bytes(&mut bytes)?;
        let mut digest = Sha256::new();
        digest.update(CHECKPOINT_ABI);
        digest.update(self.config);
        digest.update(self.frame.to_le_bytes());
        digest.update(bytes);
        Ok(digest.finalize().into())
    }

    fn persistent(&self) -> [u8; 32] {
        Sha256::digest(self.emu.dump_persistent_data().expect("fixture has SRAM")).into()
    }

    fn step(&mut self, input: ConfirmedInput) -> Result<Checkpoint> {
        ensure!(!self.closed, "machine lease is closed");
        ensure!(input.frame == self.frame, "noncontiguous machine input");
        self.emu.set_input_p1_raw(input.ports[0]);
        self.emu.set_input_p2_raw(input.ports[1]);
        self.emu.step_frame();
        self.frame += 1;
        ensure!(self.emu.frame_count() == self.frame, "core frame drift");
        let mut audio = Sha256::new();
        for sample in self.emu.drain_audio_samples() {
            audio.update(sample.to_bits().to_le_bytes());
        }
        Ok(Checkpoint {
            frame: self.frame,
            logical: self.logical()?,
            video: Sha256::digest(self.emu.framebuffer()).into(),
            audio: audio.finalize().into(),
            persistent: self.persistent(),
        })
    }

    fn close(&mut self) {
        self.closed = true;
    }
}

fn sample(player: Player, frame: u64) -> u8 {
    let phase = frame / 3;
    match player {
        Player::One => (phase.wrapping_mul(37) ^ (phase >> 3)) as u8,
        Player::Two => (phase.wrapping_mul(53).wrapping_add(7) ^ (phase >> 2)) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_consumes_each_controller_and_mutates_leased_sram() {
        let mut neutral = Machine::new().unwrap();
        let initial = neutral.persistent();
        let mut p1 = Machine::new().unwrap();
        let mut p2 = Machine::new().unwrap();
        for frame in 0..7 {
            neutral
                .step(ConfirmedInput {
                    frame,
                    ports: [0, 0],
                })
                .unwrap();
            p1.step(ConfirmedInput {
                frame,
                ports: [1, 0],
            })
            .unwrap();
            p2.step(ConfirmedInput {
                frame,
                ports: [0, 2],
            })
            .unwrap();
        }
        assert_eq!(p1.emu.cpu_read8_debuggable(0), 0x80);
        assert_eq!(p1.emu.cpu_read8_debuggable(1), 0);
        assert_eq!(p2.emu.cpu_read8_debuggable(0), 0);
        assert_eq!(p2.emu.cpu_read8_debuggable(1), 0x40);
        assert_ne!(p1.persistent(), initial);
        assert_ne!(p2.persistent(), initial);
        assert_ne!(neutral.logical().unwrap(), p1.logical().unwrap());
        assert_ne!(neutral.logical().unwrap(), p2.logical().unwrap());
        p1.close();
        assert!(
            p1.step(ConfirmedInput {
                frame: 7,
                ports: [0, 0]
            })
            .is_err()
        );
    }

    #[test]
    fn twin_core_checkpoints_match_and_inputs_change_output() {
        let mut left = Machine::new().unwrap();
        let mut right = Machine::new().unwrap();
        let mut neutral = Machine::new().unwrap();
        let mut changed = [false; 4];
        for frame in 0..300 {
            let input = ConfirmedInput {
                frame,
                ports: [sample(Player::One, frame), sample(Player::Two, frame)],
            };
            let one = left.step(input).unwrap();
            assert_eq!(one, right.step(input).unwrap(), "frame {frame}");
            let zero = neutral
                .step(ConfirmedInput {
                    frame,
                    ports: [0, 0],
                })
                .unwrap();
            changed[0] |= one.logical != zero.logical;
            changed[1] |= one.video != zero.video;
            changed[2] |= one.audio != zero.audio;
            changed[3] |= one.persistent != zero.persistent;
        }
        assert_eq!(changed, [true; 4]);
    }
}
