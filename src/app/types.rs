use std::path::PathBuf;
use std::time::Duration;

use crate::camera::CameraCapture;
use crate::platform::Instant;

pub(super) struct TiltState {
    pub(super) smoothed: (f32, f32),
    pub(super) left_stick: (f32, f32),
    pub(super) auto_source: super::tilt::AutoTiltSource,
}

pub(super) struct CameraState {
    pub(super) capture: Option<CameraCapture>,
    pub(super) capture_index: Option<u32>,
}

pub(super) struct RecycledBuffers {
    pub(super) audio: Option<Vec<f32>>,
    pub(super) vram: Option<Vec<u8>>,
    pub(super) oam: Option<Vec<u8>>,
    pub(super) memory_page: Option<Vec<(u16, u8)>>,
}

impl RecycledBuffers {
    pub(super) fn clear(&mut self) {
        self.audio = None;
        self.vram = None;
        self.oam = None;
        self.memory_page = None;
    }
}

pub(super) struct RewindState {
    pub(super) held: bool,
    pub(super) fill: f32,
    pub(super) throttle: usize,
    pub(super) pops: usize,
    pub(super) pending: bool,
    pub(super) backstep_pending: bool,
}

pub(super) struct RecordingState {
    pub(super) audio_recorder: Option<crate::audio_recorder::AudioRecorder>,
    pub(super) replay_recorder: Option<zeff_emu_common::replay::ReplayRecorder>,
    pub(super) replay_player: Option<zeff_emu_common::replay::ReplayPlayer>,
}

impl RecordingState {
    pub(super) fn is_audio_recording(&self) -> bool {
        self.audio_recorder.is_some()
    }
}

pub(super) struct TimingState {
    pub(super) last_frame_time: Instant,
    pub(super) last_render_time: Instant,
    pub(super) last_viewer_update: Instant,
    pub(super) uncapped_speed: bool,
    pub(super) last_vsync_mode: crate::settings::VsyncMode,
}

#[derive(Default)]
pub(super) struct ModifierKeys {
    pub(super) shift: bool,
    pub(super) ctrl: bool,
    pub(super) alt: bool,
}

#[derive(Default)]
pub(super) struct DebugRequests {
    pub(super) step: bool,
    pub(super) continue_: bool,
    pub(super) backstep: bool,
    pub(super) frame_advance: bool,
}

impl DebugRequests {
    pub(super) fn has_pending(&self) -> bool {
        self.step || self.continue_ || self.backstep || self.frame_advance
    }
}

pub(super) struct CachedRomInfo {
    pub(super) is_mbc7: bool,
    pub(super) is_pocket_camera: bool,
    pub(super) rom_path: Option<PathBuf>,
    pub(super) rom_hash: Option<[u8; 32]>,
}

pub(super) struct SpeedState {
    pub(super) paused: bool,
    pub(super) fast_forward_held: bool,
    pub(super) turbo_held: bool,
    pub(super) turbo_counter: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SpeedMode {
    Normal,
    Uncapped,
    FastForward,
}

pub(super) const GB_FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);
pub(super) const MAX_IN_FLIGHT: usize = 2;
pub(super) const MAX_FRAMES_PER_TICK: usize = 10;
pub(super) const UI_RENDER_INTERVAL: Duration = Duration::from_millis(16);
pub(super) const VIEWER_UPDATE_INTERVAL: Duration = Duration::from_millis(33);
pub(super) const NES_FRAME_DURATION: Duration = Duration::from_nanos(16_639_267);
