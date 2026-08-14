use std::collections::VecDeque;
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc;
use std::time::Duration;

use crate::camera::CameraCapture;
use crate::platform::Instant;
use zeff_emu_common::address::Address;

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
    pub(super) memory_page: Option<Vec<(Address, u8)>>,
    pub(super) nes_chr: Option<Vec<u8>>,
    pub(super) nes_nametable: Option<Vec<u8>>,
}

impl RecycledBuffers {
    pub(super) fn clear(&mut self) {
        self.audio = None;
        self.vram = None;
        self.oam = None;
        self.memory_page = None;
        self.nes_chr = None;
        self.nes_nametable = None;
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ReplayCaptureOrigin {
    pub(super) frame: u64,
    pub(super) game_boy_tick: Option<u64>,
}

pub(super) struct RecordingState {
    pub(super) audio_recorder: Option<crate::audio_recorder::AudioRecorder>,
    pub(super) replay_recorder: Option<zeff_emu_common::replay::ReplayRecorder>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) pending_replay_start: Option<PendingReplayStart>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) replay_finalization: Option<ReplayFinalizationState>,
    pub(super) replay_player: Option<zeff_emu_common::replay::ReplayPlayer>,
    pub(super) pending_replay_batches: VecDeque<PendingReplayBatch>,
    pub(super) queued_replay_playback_frames: usize,
    pub(super) replay_recording_origin: ReplayCaptureOrigin,
    pub(super) replay_media_events_pending: usize,
}

impl RecordingState {
    pub(super) fn is_audio_recording(&self) -> bool {
        self.audio_recorder.is_some()
    }

    pub(super) fn is_replay_active(&self) -> bool {
        self.replay_recorder.is_some()
            || self.is_replay_start_pending()
            || self.is_replay_finalizing()
            || self.replay_player.is_some()
            || !self.pending_replay_batches.is_empty()
    }

    pub(super) fn replay_recorder_for_commits(
        &mut self,
    ) -> Option<&mut zeff_emu_common::replay::ReplayRecorder> {
        if let Some(recorder) = self.replay_recorder.as_mut() {
            return Some(recorder);
        }

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(ReplayFinalizationState::CapturingFinalState { recorder, .. }) =
            self.replay_finalization.as_mut()
        {
            return Some(recorder);
        }

        None
    }

    pub(super) fn is_replay_finalizing(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.replay_finalization.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    pub(super) fn is_replay_start_pending(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending_replay_start.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn is_replay_final_state_capture_pending(&self) -> bool {
        matches!(
            self.replay_finalization,
            Some(ReplayFinalizationState::CapturingFinalState { .. })
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) struct PendingReplayStart {
    pub(super) path: PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) enum ReplayFinalizationState {
    CapturingFinalState {
        recorder: Box<zeff_emu_common::replay::ReplayRecorder>,
        frame_count: usize,
    },
    Saving {
        frame_count: usize,
        receiver: mpsc::Receiver<ReplaySaveResult>,
    },
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) struct ReplaySaveResult {
    pub(super) frame_count: usize,
    pub(super) result: Result<PathBuf, String>,
}

pub(super) struct PendingReplayBatch {
    pub(super) frames: Vec<crate::emu_thread::ReplayJoypadFrame>,
    pub(super) record: bool,
    pub(super) playback: bool,
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
    pub(super) source_path: Option<PathBuf>,
    pub(super) rom_hash: Option<[u8; 32]>,
    pub(super) replay_metadata: Option<zeff_emu_common::replay::ReplayMetadata>,
}

pub(super) type PendingArchiveSelection = crate::rom_archive::PendingArchiveSelection;

#[derive(Default)]
pub(super) struct NesPaletteFileCache {
    pub(super) path: String,
    pub(super) palette: Option<zeff_nes_core::hardware::ppu::NesPalette>,
    pub(super) error: Option<String>,
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
    SlowMotion,
    Uncapped,
    FastForward,
}

pub(super) const MAX_IN_FLIGHT: usize = 2;
pub(super) const MAX_FRAMES_PER_TICK: usize = 10;
pub(super) const UI_RENDER_INTERVAL: Duration = Duration::from_millis(16);
pub(super) const VIEWER_UPDATE_INTERVAL: Duration = Duration::from_millis(33);

#[cfg(test)]
mod tests {
    use super::*;

    fn recording_state() -> RecordingState {
        RecordingState {
            audio_recorder: None,
            replay_recorder: None,
            pending_replay_start: None,
            replay_finalization: None,
            replay_player: None,
            pending_replay_batches: VecDeque::new(),
            queued_replay_playback_frames: 0,
            replay_recording_origin: ReplayCaptureOrigin::default(),
            replay_media_events_pending: 0,
        }
    }

    #[test]
    fn replay_activity_includes_recording_and_pending_batches() {
        let mut state = recording_state();
        assert!(!state.is_replay_active());

        state.replay_recorder = Some(zeff_emu_common::replay::ReplayRecorder::new(
            PathBuf::from("test.zrpl"),
            vec![],
        ));
        assert!(state.is_replay_active());

        state.replay_recorder = None;
        state.pending_replay_start = Some(PendingReplayStart {
            path: PathBuf::from("starting.zrpl"),
        });
        assert!(state.is_replay_active());

        state.pending_replay_start = None;
        state.pending_replay_batches.push_back(PendingReplayBatch {
            frames: Vec::new(),
            record: false,
            playback: true,
        });
        assert!(state.is_replay_active());

        #[cfg(not(target_arch = "wasm32"))]
        {
            state.pending_replay_batches.clear();
            let (_sender, receiver) = mpsc::channel();
            state.replay_finalization = Some(ReplayFinalizationState::Saving {
                frame_count: 0,
                receiver,
            });
            assert!(state.is_replay_active());
        }
    }
}
