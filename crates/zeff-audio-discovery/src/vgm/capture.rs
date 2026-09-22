use std::sync::atomic::AtomicBool;

use anyhow::Result;
use serde::Serialize;
use zeff_emu_common::audio_trace::{
    AudioTrace, Huc6280AudioTrace, NesAudioTrace, WonderSwanAudioTrace,
};

mod game_boy;
mod huc6280;
pub use game_boy::{GameBoyVgmExport, GameBoyVgmUnavailable, encode_game_boy};
mod nes;
pub use nes::{NesVgmExport, NesVgmUnavailable};
mod sn76489;
mod stream;
mod wonderswan;

pub const VGM_VERSION: u32 = 0x171;
const HEADER_LEN: usize = 0x100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VgmCapture {
    pub bytes: Vec<u8>,
    pub metadata: VgmCaptureMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VgmCaptureMetadata {
    pub schema: &'static str,
    pub version: u32,
    pub trace_generation: u64,
    pub timing: &'static str,
    pub cycle_hz: u32,
    #[serde(skip_serializing_if = "is_one")]
    pub cycle_hz_denominator: u32,
    pub end_cycle: u64,
    pub total_samples: u32,
    pub quantization: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sn76489_flags: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub huc6280: Option<Huc6280CaptureMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wonder_swan: Option<WonderSwanCaptureMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_boy: Option<GameBoyCaptureMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nes: Option<NesCaptureMetadata>,
    pub preamble_write_count: u32,
    pub guest_write_count: u32,
    pub wait_command_count: u32,
    pub command_count: u32,
    pub limitations: &'static [&'static str],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Huc6280CaptureMetadata {
    pub clock_hz_numerator: u64,
    pub clock_hz_denominator: u32,
    pub vgm_clock_hz: u32,
    pub clock_rounding: &'static str,
    pub revision: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WonderSwanCaptureMetadata {
    pub model: &'static str,
    pub master_clock_hz: u32,
    pub wave_ram_bytes: u32,
    pub canonical_reset: &'static str,
    pub hyper_voice: &'static str,
    pub sound_test: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GameBoyCaptureMetadata {
    pub model: &'static str,
    pub master_clock_hz: u32,
    pub reset: &'static str,
    pub observed_timing_event_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NesCaptureMetadata {
    pub region: &'static str,
    pub clock_hz_numerator: u64,
    pub clock_hz_denominator: u32,
    pub vgm_clock_hz: u32,
    pub clock_rounding: &'static str,
    pub reset: &'static str,
    pub status_read_observation_count: u32,
}

fn is_one(value: &u32) -> bool {
    *value == 1
}

pub trait VgmCaptureSource {
    fn encode_vgm(&self, cancel: &AtomicBool) -> Result<VgmCapture>;
}

impl VgmCaptureSource for AudioTrace {
    fn encode_vgm(&self, cancel: &AtomicBool) -> Result<VgmCapture> {
        encode_sn76489(self, cancel)
    }
}

impl VgmCaptureSource for Huc6280AudioTrace {
    fn encode_vgm(&self, cancel: &AtomicBool) -> Result<VgmCapture> {
        encode_huc6280(self, cancel)
    }
}

impl VgmCaptureSource for WonderSwanAudioTrace {
    fn encode_vgm(&self, cancel: &AtomicBool) -> Result<VgmCapture> {
        encode_wonderswan(self, cancel)
    }
}

pub fn encode_sn76489(trace: &AudioTrace, cancel: &AtomicBool) -> Result<VgmCapture> {
    sn76489::encode(trace, cancel)
}

pub fn encode_huc6280(trace: &Huc6280AudioTrace, cancel: &AtomicBool) -> Result<VgmCapture> {
    huc6280::encode(trace, cancel)
}

pub fn encode_wonderswan(trace: &WonderSwanAudioTrace, cancel: &AtomicBool) -> Result<VgmCapture> {
    wonderswan::encode(trace, cancel)
}

pub fn encode_nes(trace: &NesAudioTrace, cancel: &AtomicBool) -> Result<NesVgmExport> {
    nes::encode(trace, cancel)
}

#[cfg(test)]
mod game_boy_tests;
#[cfg(test)]
mod huc6280_tests;
#[cfg(test)]
mod nes_tests;
#[cfg(test)]
mod sn76489_tests;
#[cfg(test)]
mod wonderswan_tests;
