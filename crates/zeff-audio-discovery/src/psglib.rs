use std::sync::atomic::AtomicBool;

use anyhow::Result;
use serde::Serialize;

use crate::tracker::FileSpan;

mod discovery;
#[cfg(test)]
mod discovery_tests;
mod stream;
#[cfg(test)]
mod tests;
mod vgm;

pub use discovery::{
    BoundSequence, DiscoveryReport, ExecutableEvidence, HeldEvidence, HeldKind, TableEntryEvidence,
    discover,
};

pub const SOURCE_REVISION: &str = "f433a35d26337efe8b18a72fdcc9ac1ef0713f74";
pub const MAX_STREAM_BYTES: usize = 0x4000;
pub const MAX_FRAMES: u32 = 216_000;
pub const MAX_WRITES: usize = 262_144;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameRate {
    Hz50,
    Hz60,
}

impl FrameRate {
    pub fn hz(self) -> u32 {
        match self {
            Self::Hz50 => 50,
            Self::Hz60 => 60,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FrameWrite {
    pub frame: u32,
    pub value: u8,
    pub source_offset: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DecodedStream {
    pub writes: Vec<FrameWrite>,
    pub frames: u32,
    pub spans: Vec<FileSpan>,
    pub loop_offset: Option<u32>,
    pub end_offset: u32,
}

pub fn decode(bytes: &[u8], offset: u32, cancel: &AtomicBool) -> Result<DecodedStream> {
    stream::decode(bytes, offset, cancel)
}

pub fn export_vgm(
    bytes: &[u8],
    offset: u32,
    rate: FrameRate,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    vgm::encode(&decode(bytes, offset, cancel)?, rate, cancel)
}
