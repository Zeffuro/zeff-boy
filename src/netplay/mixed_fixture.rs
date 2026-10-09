use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeff_netplay::wire::Message;

pub(crate) const FRAMES: usize = 72;
pub(crate) const MAX_FRAMES: usize = 96;

pub(crate) fn media(timing: u8) -> Vec<u8> {
    let mut bytes = zeff_netplay::fixture::rom();
    bytes[7] = 0x28;
    bytes[10] = 0x70;
    bytes[12] = timing;
    bytes
}

pub(crate) fn input(frame: u64, player: usize) -> u16 {
    let shift = ((frame / 3 + player as u64 * 3) % 8) as u32;
    u16::from((1u8 << shift) ^ (frame.is_multiple_of(5) as u8 * 0x18))
}

pub(crate) fn ports(frame: u64, delay: u64) -> [u16; 2] {
    if frame < delay {
        [0; 2]
    } else {
        [input(frame - delay, 0), input(frame - delay, 1)]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) frame: u64,
    pub(crate) ports: [u16; 2],
    logical: [u8; 32],
    video: [u8; 32],
    audio: [u8; 32],
    persistent: [u8; 32],
    samples: usize,
}

impl Frame {
    pub(crate) fn new(checkpoint: &Message, ports: [u16; 2], pcm: &[f32]) -> Result<Self> {
        let Message::Checkpoint {
            frame,
            logical,
            video,
            audio,
            persistent,
        } = checkpoint
        else {
            anyhow::bail!("expected frame checkpoint");
        };
        let mut digest = Sha256::new();
        for sample in pcm {
            digest.update(sample.to_bits().to_le_bytes());
        }
        ensure!(
            *audio == <[u8; 32]>::from(digest.finalize()),
            "checkpoint PCM differs"
        );
        Ok(Self {
            frame: *frame,
            ports,
            logical: *logical,
            video: *video,
            audio: *audio,
            persistent: *persistent,
            samples: pcm.len(),
        })
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Progress {
    pub(crate) ready: bool,
    pub(crate) presented: u64,
    pub(crate) pending: bool,
    pub(crate) verified: usize,
    pub(crate) replayed: u64,
    pub(crate) restored: bool,
    pub(crate) published: bool,
    pub(crate) closed_at: u64,
    pub(crate) stop_reason: Option<String>,
    pub(crate) finished: bool,
    pub(crate) error: Option<String>,
    pub(crate) frames: Vec<Frame>,
}

#[cfg(not(target_arch = "wasm32"))]
mod native;

#[test]
fn mixed_receipt_rejects_pcm_and_checkpoint_disagreement() {
    let checkpoint = Message::Checkpoint {
        frame: 1,
        logical: [1; 32],
        video: [2; 32],
        audio: Sha256::digest([]).into(),
        persistent: [3; 32],
    };
    assert!(Frame::new(&checkpoint, [0; 2], &[]).is_ok());
    assert!(Frame::new(&checkpoint, [0; 2], &[-0.0]).is_err());
    assert!(Frame::new(&Message::Close { frame: 1 }, [0; 2], &[]).is_err());
}
