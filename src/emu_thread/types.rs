use std::path::PathBuf;
use std::sync::Arc;

use arc_swap::ArcSwapOption;

use crate::debug::DebugUiActions;
use crate::ui;
use zeff_emu_common::address::Address;

pub(crate) type SharedFramebuffer = Arc<ArcSwapOption<Vec<u8>>>;

pub(crate) fn new_shared_framebuffer() -> SharedFramebuffer {
    Arc::new(ArcSwapOption::empty())
}

pub(crate) fn publish_framebuffer(shared_fb: &SharedFramebuffer, framebuffer: &[u8]) {
    shared_fb.store(Some(Arc::new(framebuffer.to_vec())));
}

pub(crate) fn publish_owned_framebuffer(shared_fb: &SharedFramebuffer, framebuffer: Vec<u8>) {
    shared_fb.store(Some(Arc::new(framebuffer)));
}

#[cfg(feature = "profile-cores")]
pub(crate) fn profile_frame_publication(framebuffer: &[u8], iterations: u32) {
    use std::hint::black_box;
    use std::time::Instant;

    let shared = new_shared_framebuffer();
    for _ in 0..10 {
        publish_framebuffer(&shared, framebuffer);
    }
    let start = Instant::now();
    for _ in 0..iterations {
        publish_framebuffer(&shared, black_box(framebuffer));
    }
    let elapsed = start.elapsed();
    let frames_per_second = f64::from(iterations) / elapsed.as_secs_f64();
    black_box(shared.load_full());
    println!(
        "frame publication                {iterations:5} frames  {elapsed:>9.2?}  {frames_per_second:>8.1} fps"
    );
}

pub(crate) struct RenderSettings {
    pub(crate) color_correction: crate::settings::ColorCorrection,
    pub(crate) color_correction_matrix: [f32; 9],
    pub(crate) dmg_palette_preset: crate::settings::DmgPalettePreset,
    pub(crate) nes_palette_mode: crate::settings::NesPaletteMode,
    pub(crate) nes_custom_palette: Option<zeff_nes_core::hardware::ppu::NesPalette>,
    pub(crate) pce_overscan_mode: crate::settings::PceOverscanMode,
    pub(crate) pce_palette_mode: crate::settings::PcePaletteMode,
    pub(crate) sgb_border_enabled: bool,
}

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
    pub(crate) memory_view_start: Address,
    pub(crate) show_rom_viewer: bool,
    pub(crate) show_instruction_trace: bool,
    pub(crate) trace_after_sequence: Option<u64>,
    pub(crate) rom_view_start: u32,
    pub(crate) last_disasm_pc: Option<Address>,
    pub(crate) last_disasm_mapping: Option<u64>,
    pub(crate) disasm_target: Option<crate::debug::DisassemblyTarget>,
    pub(crate) memory_search: Option<MemorySearchRequest>,
    pub(crate) rom_search: Option<MemorySearchRequest>,
    pub(crate) render: RenderSettings,
}

pub(crate) struct MemorySearchRequest {
    pub(crate) pattern: Vec<u8>,
    pub(crate) max_results: usize,
}

pub(crate) struct ReusableBuffers {
    pub(crate) audio: Option<Vec<f32>>,
    pub(crate) vram: Option<Vec<u8>>,
    pub(crate) oam: Option<Vec<u8>>,
    pub(crate) memory_page: Option<Vec<(Address, u8)>>,
    pub(crate) nes_chr: Option<Vec<u8>>,
    pub(crate) nes_nametable: Option<Vec<u8>>,
}

pub(crate) struct JoypadInput {
    pub(crate) buttons: u8,
    pub(crate) dpad: u8,
    pub(crate) buttons_p2: u8,
    pub(crate) dpad_p2: u8,
    pub(crate) buttons_p3: u8,
    pub(crate) dpad_p3: u8,
    pub(crate) buttons_p4: u8,
    pub(crate) dpad_p4: u8,
    pub(crate) buttons_p5: u8,
    pub(crate) dpad_p5: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PceMouseInput {
    pub(crate) mode: zeff_pce_core::hardware::PceControllerMode,
    pub(crate) memory_base_mode: zeff_pce_core::hardware::PceMemoryBaseMode,
    pub(crate) delta_x: i16,
    pub(crate) delta_y: i16,
    pub(crate) buttons: u8,
}

pub(crate) type ReplayJoypadFrame = zeff_emu_common::replay::ReplayJoypadFrame;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ZapperInput {
    pub(crate) enabled: bool,
    pub(crate) trigger: bool,
    pub(crate) hit: bool,
    pub(crate) screen_pos: Option<(u16, u16)>,
}

impl From<ZapperInput> for zeff_emu_common::replay::ReplayZapperFrame {
    fn from(value: ZapperInput) -> Self {
        Self {
            enabled: value.enabled,
            trigger: value.trigger,
            hit: value.hit,
            screen_pos: value.screen_pos,
        }
    }
}

impl From<zeff_emu_common::replay::ReplayZapperFrame> for ZapperInput {
    fn from(value: zeff_emu_common::replay::ReplayZapperFrame) -> Self {
        Self {
            enabled: value.enabled,
            trigger: value.trigger,
            hit: value.hit,
            screen_pos: value.screen_pos,
        }
    }
}

pub(crate) struct AudioConfig {
    pub(crate) apu_capture_enabled: bool,
    pub(crate) skip_audio: bool,
    pub(crate) playback_speed: usize,
    pub(crate) recording_capture: AudioRecordingCapture,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AudioRecordingCapture {
    pub(crate) active: bool,
    pub(crate) semantic: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct GuestCallRequest {
    pub(crate) name: String,
    pub(crate) target: u32,
    pub(crate) storage_offset: Option<u64>,
    pub(crate) explicit_overlay: bool,
    pub(crate) exec_mode: crate::symbols::ExecMode,
    pub(crate) instruction_budget: u64,
}

pub(crate) struct FrameInput {
    pub(crate) frames: usize,
    pub(crate) replay_joypad_frames: Option<Vec<ReplayJoypadFrame>>,
    pub(crate) host_tilt: (f32, f32),
    pub(crate) host_camera_frame: Option<Vec<u8>>,
    pub(crate) joypad: JoypadInput,
    pub(crate) pce_mouse: PceMouseInput,
    pub(crate) zapper: ZapperInput,
    pub(crate) debug_step: bool,
    pub(crate) debug_continue: bool,
    pub(crate) debug_suspend_after_frame: bool,
    pub(crate) audio: AudioConfig,
    pub(crate) debug_actions: DebugUiActions,
    pub(crate) snapshot: SnapshotRequest,
    pub(crate) buffers: ReusableBuffers,
    pub(crate) rewind_enabled: bool,
    pub(crate) rewind_seconds: usize,
}

pub(crate) struct FrameResult {
    pub(crate) advanced_frames: usize,
    pub(crate) delivery_merged: bool,
    pub(crate) replay_events: Vec<zeff_emu_common::replay::ReplayEvent>,
    pub(crate) replay_error: Option<String>,
    pub(crate) runtime_fault: Option<String>,
    pub(crate) rumble: bool,
    pub(crate) audio_samples: Vec<f32>,
    pub(crate) audio_playback_speed: usize,
    pub(crate) ui_data: ui::UiFrameData,
    pub(crate) is_mbc7: bool,
    pub(crate) is_pocket_camera: bool,
    pub(crate) game_boy_serial_device: Option<zeff_gb_core::hardware::GameBoySerialDevice>,
    pub(crate) game_boy_printer_jobs: Vec<zeff_gb_core::hardware::GameBoyPrinterJob>,
    pub(crate) media_slot_snapshot: Option<zeff_emu_common::media::MediaSlotSnapshot>,
    pub(crate) rewind_fill: f32,
    pub(crate) audio_semantic_frames: Vec<crate::audio_tooling::AudioSemanticFrame>,
    pub(crate) audio_timeline_discontinuities:
        Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
}

#[derive(Default)]
pub(crate) struct WorkerRuntimeFault {
    faulted: bool,
    pending_delivery: Option<String>,
}

impl WorkerRuntimeFault {
    pub(crate) fn can_step(&self) -> bool {
        !self.faulted
    }

    pub(crate) fn latch(&mut self, fault: Option<String>) -> bool {
        let Some(fault) = fault else {
            return false;
        };
        self.faulted = true;
        if self.pending_delivery.is_none() {
            self.pending_delivery = Some(fault);
        }
        true
    }

    pub(crate) fn take_pending_delivery(&mut self) -> Option<String> {
        self.pending_delivery.take()
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TcpLinkMode {
    Host { bind_addr: String },
    Join { connect_addr: String },
}

pub(crate) enum EmuCommand {
    StepFrames(Box<FrameInput>),
    SetAudioRecordingCapture {
        capture: AudioRecordingCapture,
        acknowledged: Option<std::sync::mpsc::Sender<()>>,
    },
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
    ExecuteGuestCall(GuestCallRequest),
    UndoGuestCall(Vec<u8>),
    CaptureReplayStart {
        capture_id: u64,
    },
    CaptureReplayCheckpoint {
        frame: u64,
    },
    LoadStateBytes {
        state_bytes: Vec<u8>,
        buttons_pressed: u8,
        dpad_pressed: u8,
        replay_events: Option<Vec<zeff_emu_common::replay::ReplayEvent>>,
        game_boy_link_start_state: Option<zeff_emu_common::replay::ReplayGameBoyLinkState>,
        game_boy_link_coordinator_start_state:
            Option<zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorState>,
        game_boy_link_start_tick: Option<u64>,
        wonder_swan_link_start_tick: Option<u64>,
    },
    SetSampleRate(u32),
    SetUncapped(bool),
    SetUncappedBatchSize(usize),
    ApplyMediaEvent(zeff_emu_common::media::MediaEvent),
    SetGameBoySerialDevice(zeff_gb_core::hardware::GameBoySerialDevice),
    QueueBardigunBarcodeScan(Vec<u8>),
    TriggerBarcodeBoyScan(String),
    RestoreGameBoyLinkState(zeff_emu_common::replay::ReplayGameBoyLinkState),
    UpdateCheats(Vec<crate::cheats::CheatPatch>),
    Reset,
    #[cfg(not(target_arch = "wasm32"))]
    StartTcpLink(TcpLinkMode),
    #[cfg(not(target_arch = "wasm32"))]
    DisconnectLink,
    Rewind(usize),
    Shutdown,
}

pub(crate) enum EmuResponse {
    SaveStateOk(String),
    SaveStateFailed(String),
    LoadStateOk {
        path: String,
        media_slot_snapshot: Option<zeff_emu_common::media::MediaSlotSnapshot>,
        game_boy_serial_device: Option<zeff_gb_core::hardware::GameBoySerialDevice>,
    },
    LoadStateFailed(String),
    RewindOk {
        media_slot_snapshot: Option<zeff_emu_common::media::MediaSlotSnapshot>,
        game_boy_serial_device: Option<zeff_gb_core::hardware::GameBoySerialDevice>,
        rewound_frames: u64,
    },
    RewindFailed(String),
    StateCaptured(Vec<u8>),
    ReplayStartCaptured {
        capture_id: u64,
        start: Box<ReplayStartState>,
    },
    ReplayStartCaptureFailed {
        capture_id: u64,
        error: String,
    },
    ReplayCheckpointCaptured {
        frame: u64,
        state_bytes: Vec<u8>,
    },
    ReplayCheckpointCaptureFailed {
        frame: u64,
        error: String,
    },
    StateCaptureFailed(String),
    GuestCallCompleted {
        name: String,
        instructions: u64,
        undo_state: Vec<u8>,
    },
    GuestCallFailed {
        name: String,
        error: String,
    },
    GuestCallUndone,
    GuestCallUndoFailed(String),
    MediaEventApplied {
        event: zeff_emu_common::media::MediaEvent,
        snapshot: zeff_emu_common::media::MediaSlotSnapshot,
        frame_count: u64,
    },
    MediaEventFailed {
        event: zeff_emu_common::media::MediaEvent,
        error: String,
    },
    BardigunBarcodeScanStarted(usize),
    BardigunBarcodeScanFailed(String),
    BarcodeBoyScanStarted,
    BarcodeBoyScanFailed(String),
    #[cfg(not(target_arch = "wasm32"))]
    LinkPending(String),
    #[cfg(not(target_arch = "wasm32"))]
    LinkConnected {
        label: String,
        frame_count: u64,
        game_boy_cpu_cycles: Option<u64>,
        game_boy_link_state: Option<zeff_emu_common::replay::ReplayGameBoyLinkState>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    LinkFailed(String),
    #[cfg(not(target_arch = "wasm32"))]
    LinkDisconnected {
        frame_count: u64,
        game_boy_cpu_cycles: Option<u64>,
        game_boy_link_state: Option<zeff_emu_common::replay::ReplayGameBoyLinkState>,
    },
    SramFlushed(Option<String>),
    ShutdownComplete,
}

pub(crate) struct ReplayStartState {
    pub(crate) state_bytes: Vec<u8>,
    pub(crate) frame_count: u64,
    pub(crate) game_boy_cpu_cycles: Option<u64>,
    pub(crate) wonder_swan_cpu_cycles: Option<u64>,
    pub(crate) metadata: zeff_emu_common::replay::ReplayMetadata,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_framebuffer_stores_an_owned_snapshot() {
        let shared_fb = new_shared_framebuffer();
        let mut source = vec![1, 2, 3, 4];

        publish_framebuffer(&shared_fb, &source);
        source.fill(0);

        let stored = shared_fb.load_full().expect("framebuffer should be stored");
        assert_eq!(&**stored, &[1, 2, 3, 4]);
    }

    #[test]
    fn publish_owned_framebuffer_stores_the_given_buffer() {
        let shared_fb = new_shared_framebuffer();

        publish_owned_framebuffer(&shared_fb, vec![5, 6, 7, 8]);

        let stored = shared_fb.load_full().expect("framebuffer should be stored");
        assert_eq!(&**stored, &[5, 6, 7, 8]);
    }

    #[test]
    fn runtime_fault_is_one_shot_but_keeps_worker_faulted() {
        let mut fault = WorkerRuntimeFault::default();
        assert!(fault.can_step());

        assert!(fault.latch(Some("first".to_string())));
        assert!(fault.latch(Some("second".to_string())));
        assert!(!fault.can_step());
        assert_eq!(fault.take_pending_delivery().as_deref(), Some("first"));
        assert_eq!(fault.take_pending_delivery(), None);
        assert!(!fault.can_step());
    }
}
