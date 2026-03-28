use std::path::PathBuf;

use crate::debug::DebugUiActions;
use crate::ui;

pub(crate) struct SnapshotRequest {
    pub(crate) want_debug_info: bool,
    pub(crate) want_perf_info: bool,
    pub(crate) any_viewer_open: bool,
    pub(crate) any_vram_viewer_open: bool,
    pub(crate) show_oam_viewer: bool,
    pub(crate) show_apu_viewer: bool,
    pub(crate) show_disassembler: bool,
    pub(crate) show_rom_info: bool,
    pub(crate) show_memory_viewer: bool,
    pub(crate) memory_view_start: u16,
    pub(crate) show_rom_viewer: bool,
    pub(crate) rom_view_start: u32,
    pub(crate) last_disasm_pc: Option<u16>,
    pub(crate) memory_search: Option<MemorySearchRequest>,
    pub(crate) rom_search: Option<MemorySearchRequest>,
    pub(crate) color_correction: crate::settings::ColorCorrection,
    pub(crate) color_correction_matrix: [f32; 9],
}

pub(crate) struct MemorySearchRequest {
    pub(crate) pattern: Vec<u8>,
    pub(crate) max_results: usize,
}

pub(crate) struct ReusableBuffers {
    pub(crate) framebuffer: Option<Vec<u8>>,
    pub(crate) audio: Option<Vec<f32>>,
    pub(crate) vram: Option<Vec<u8>>,
    pub(crate) oam: Option<Vec<u8>>,
    pub(crate) memory_page: Option<Vec<(u16, u8)>>,
}

pub(crate) struct FrameInput {
    pub(crate) frames: usize,
    pub(crate) host_tilt: (f32, f32),
    pub(crate) host_camera_frame: Option<Vec<u8>>,
    pub(crate) buttons_pressed: u8,
    pub(crate) dpad_pressed: u8,
    pub(crate) buttons_pressed_p2: u8,
    pub(crate) dpad_pressed_p2: u8,
    pub(crate) debug_step: bool,
    pub(crate) debug_continue: bool,
    pub(crate) apu_capture_enabled: bool,
    pub(crate) skip_audio: bool,
    pub(crate) midi_capture_active: bool,
    pub(crate) debug_actions: DebugUiActions,
    pub(crate) snapshot: SnapshotRequest,
    pub(crate) buffers: ReusableBuffers,
    pub(crate) rewind_enabled: bool,
    pub(crate) rewind_seconds: usize,
}

pub(crate) struct FrameResult {
    pub(crate) frame: Vec<u8>,
    pub(crate) rumble: bool,
    pub(crate) audio_samples: Vec<f32>,
    pub(crate) ui_data: ui::UiFrameData,
    pub(crate) is_mbc7: bool,
    pub(crate) is_pocket_camera: bool,
    pub(crate) rewind_fill: f32,
    pub(crate) apu_snapshot: Option<crate::audio_recorder::MidiApuSnapshot>,
}

pub(crate) enum EmuCommand {
    StepFrames(Box<FrameInput>),
    SaveStateSlot(u8),
    LoadStateSlot {
        slot: u8,
        buttons_pressed: u8,
        dpad_pressed: u8,
    },
    SaveStateToPath(PathBuf),
    LoadStateFromPath {
        path: PathBuf,
        buttons_pressed: u8,
        dpad_pressed: u8,
    },
    AutoSaveState,
    AutoLoadState {
        buttons_pressed: u8,
        dpad_pressed: u8,
    },
    CaptureStateBytes,
    LoadStateBytes {
        state_bytes: Vec<u8>,
        buttons_pressed: u8,
        dpad_pressed: u8,
    },
    SetSampleRate(u32),
    SetUncapped(bool),
    UpdateCheats(Vec<crate::cheats::CheatPatch>),
    Rewind,
    Shutdown,
}

pub(crate) enum EmuResponse {
    SaveStateOk(String),
    SaveStateFailed(String),
    LoadStateOk { path: String, framebuffer: Vec<u8> },
    LoadStateFailed(String),
    RewindOk { framebuffer: Vec<u8> },
    RewindFailed(String),
    StateCaptured(Vec<u8>),
    StateCaptureFailed(String),
    SramFlushed(Option<String>),
    ShutdownComplete,
}

