use serde::Serialize;

pub const CDDA_SAMPLE_RATE: u32 = 44_100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CdAudioTrack {
    pub number: u8,
    pub index1_lba: u32,
    pub end_lba: u32,
    pub pregap_start_lba: Option<u32>,
    pub sectors: u32,
    pub pcm_frames: u64,
}
