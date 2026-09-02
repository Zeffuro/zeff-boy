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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SpeculationBlockers {
    feature_disabled: bool,
    replay_timeline_active: bool,
    live_control_active: bool,
}

impl SpeculationBlockers {
    pub(crate) const fn feature_disabled() -> Self {
        Self {
            feature_disabled: true,
            replay_timeline_active: false,
            live_control_active: false,
        }
    }

    #[cfg(test)]
    pub(crate) const fn from_app_for_test(
        replay_timeline_active: bool,
        live_control_active: bool,
    ) -> Self {
        Self {
            feature_disabled: false,
            replay_timeline_active,
            live_control_active,
        }
    }

    pub(crate) fn any(self) -> bool {
        self.feature_disabled || self.replay_timeline_active || self.live_control_active
    }
}

pub(crate) struct FrameInput {
    pub(crate) frames: usize,
    pub(crate) speculation_blockers: SpeculationBlockers,
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

#[cfg(not(target_arch = "wasm32"))]
mod tas;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use tas::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasControlCommandKind {
    FrameExecution,
    AudioOrTimingConfiguration,
    StateOrRecovery,
    Replay,
    DebuggerMutation,
    MediaOrPeripheral,
    CheatConfiguration,
    Reset,
    Link,
    Rewind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EmuCommandAuthority {
    Gameplay(TasControlCommandKind),
    ObserveTasReadiness,
    TasRepair,
    AcquireTasControl,
    ExecuteTasControl,
    AdvanceTasControl,
    RollbackTasControl,
    CommitTasControl,
    Shutdown,
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
    InspectRecovery {
        resume: bool,
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
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    SuspendTasRepair {
        identity: TasRepairIdentity,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    ResumeTasRepair {
        identity: TasRepairIdentity,
        expected_proof: Box<TasRepairSuspensionProof>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    DiscardTasRepair {
        identity: TasRepairIdentity,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    CommitRepairedTasWorker {
        identity: TasRepairIdentity,
        save_recovery_on_shutdown: bool,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    DiscardRepairedTasWorker {
        identity: TasRepairIdentity,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    InspectTasReadiness {
        request_id: u64,
        profile: TasExecutionProfile,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    AcquireTasControl {
        request_id: u64,
        profile: TasExecutionProfile,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    ExecuteTasControl(Box<TasExecutionRequest>),
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    AdvanceTasControl(Box<TasFrameAdvanceRequest>),
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    RollbackTasControl {
        lease_id: u64,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    CommitTasControl {
        lease_id: u64,
    },
    #[cfg(target_arch = "wasm32")]
    FlushBatterySram,
    #[cfg(target_arch = "wasm32")]
    RestoreStateBackup(PathBuf),
    Shutdown,
}

impl EmuCommand {
    pub(crate) fn authority_classification(&self) -> EmuCommandAuthority {
        match self {
            Self::StepFrames(_) => {
                EmuCommandAuthority::Gameplay(TasControlCommandKind::FrameExecution)
            }
            Self::SetAudioRecordingCapture { .. }
            | Self::SetSampleRate(_)
            | Self::SetUncapped(_)
            | Self::SetUncappedBatchSize(_) => {
                EmuCommandAuthority::Gameplay(TasControlCommandKind::AudioOrTimingConfiguration)
            }
            Self::SaveStateSlot(_)
            | Self::LoadStateSlot { .. }
            | Self::SaveStateToPath(_)
            | Self::LoadStateFromPath { .. }
            | Self::InspectRecovery { .. }
            | Self::CaptureStateBytes
            | Self::LoadStateBytes { .. } => {
                EmuCommandAuthority::Gameplay(TasControlCommandKind::StateOrRecovery)
            }
            #[cfg(target_arch = "wasm32")]
            Self::FlushBatterySram | Self::RestoreStateBackup(_) => {
                EmuCommandAuthority::Gameplay(TasControlCommandKind::StateOrRecovery)
            }
            Self::CaptureReplayStart { .. } | Self::CaptureReplayCheckpoint { .. } => {
                EmuCommandAuthority::Gameplay(TasControlCommandKind::Replay)
            }
            Self::ExecuteGuestCall(_) | Self::UndoGuestCall(_) => {
                EmuCommandAuthority::Gameplay(TasControlCommandKind::DebuggerMutation)
            }
            Self::ApplyMediaEvent(_)
            | Self::SetGameBoySerialDevice(_)
            | Self::QueueBardigunBarcodeScan(_)
            | Self::TriggerBarcodeBoyScan(_)
            | Self::RestoreGameBoyLinkState(_) => {
                EmuCommandAuthority::Gameplay(TasControlCommandKind::MediaOrPeripheral)
            }
            Self::UpdateCheats(_) => {
                EmuCommandAuthority::Gameplay(TasControlCommandKind::CheatConfiguration)
            }
            Self::Reset => EmuCommandAuthority::Gameplay(TasControlCommandKind::Reset),
            #[cfg(not(target_arch = "wasm32"))]
            Self::StartTcpLink(_) | Self::DisconnectLink => {
                EmuCommandAuthority::Gameplay(TasControlCommandKind::Link)
            }
            Self::Rewind(_) => EmuCommandAuthority::Gameplay(TasControlCommandKind::Rewind),
            #[cfg(not(target_arch = "wasm32"))]
            Self::SuspendTasRepair { .. }
            | Self::ResumeTasRepair { .. }
            | Self::DiscardTasRepair { .. }
            | Self::CommitRepairedTasWorker { .. }
            | Self::DiscardRepairedTasWorker { .. } => EmuCommandAuthority::TasRepair,
            #[cfg(not(target_arch = "wasm32"))]
            Self::InspectTasReadiness { .. } => EmuCommandAuthority::ObserveTasReadiness,
            #[cfg(not(target_arch = "wasm32"))]
            Self::AcquireTasControl { .. } => EmuCommandAuthority::AcquireTasControl,
            #[cfg(not(target_arch = "wasm32"))]
            Self::ExecuteTasControl(_) => EmuCommandAuthority::ExecuteTasControl,
            #[cfg(not(target_arch = "wasm32"))]
            Self::AdvanceTasControl(_) => EmuCommandAuthority::AdvanceTasControl,
            #[cfg(not(target_arch = "wasm32"))]
            Self::RollbackTasControl { .. } => EmuCommandAuthority::RollbackTasControl,
            #[cfg(not(target_arch = "wasm32"))]
            Self::CommitTasControl { .. } => EmuCommandAuthority::CommitTasControl,
            Self::Shutdown => EmuCommandAuthority::Shutdown,
        }
    }
}

pub(crate) enum EmuResponse {
    SaveStateOk {
        path: PathBuf,
        backup_created: bool,
    },
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
    #[allow(dead_code)]
    TasRepairSuspended {
        proof: Box<TasRepairSuspensionProof>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasRepairSuspendRejected {
        identity: TasRepairIdentity,
        reason: TasRepairSuspendRejectedReason,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasRepairOriginalResumed {
        proof: Box<TasRepairSuspensionProof>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasRepairOriginalDiscarded {
        identity: TasRepairIdentity,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasRepairRepairedWorkerCommitted {
        identity: Box<TasRepairIdentity>,
        publication: TasPersistencePublicationOutcome,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasRepairRepairedWorkerDiscarded {
        identity: TasRepairIdentity,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasRepairActionRejected {
        identity: TasRepairIdentity,
        action: TasRepairAction,
        reason: TasRepairActionRejectedReason,
    },
    #[cfg(not(target_arch = "wasm32"))]
    LinkDisconnected {
        frame_count: u64,
        game_boy_cpu_cycles: Option<u64>,
        game_boy_link_state: Option<zeff_emu_common::replay::ReplayGameBoyLinkState>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasReadinessObserved {
        request_id: u64,
        observation: Box<TasLoadedProfileObservation>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasControlAcquired {
        request_id: u64,
        lease_id: u64,
        witness: Box<TasControlLeaseWitness>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasControlAcquireRejected {
        request_id: u64,
        reason: TasControlAcquireRejectedReason,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasExecutionCompleted {
        profile: TasExecutionProfile,
        lease_id: u64,
        run_id: u64,
        segment_id: u64,
        segment_frame_count: u64,
        executed_project_frames: u64,
        frame_count: u64,
        state_sha256: crate::tas_project::TasDigest,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasExecutionRejected {
        profile: TasExecutionProfile,
        requested_lease_id: u64,
        run_id: u64,
        reason: TasExecutionRejectedReason,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasFrameAdvanced {
        profile: TasExecutionProfile,
        lease_id: u64,
        run_id: u64,
        advance_id: u64,
        segment_id: u64,
        segment_frame_count: u64,
        executed_project_frames: u64,
        frame_count: u64,
        state_sha256: crate::tas_project::TasDigest,
        rumble: bool,
        audio_samples: Vec<f32>,
        ui_data: Option<Box<ui::UiFrameData>>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasFrameAdvanceRejected {
        profile: TasExecutionProfile,
        requested_lease_id: u64,
        run_id: u64,
        advance_id: u64,
        segment_id: u64,
        reason: TasFrameAdvanceRejectedReason,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasControlCommandRejected {
        lease_id: u64,
        command: TasControlCommandKind,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasControlRolledBack {
        lease_id: u64,
        restored_state_sha256: crate::tas_project::TasDigest,
        frame_count: u64,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasControlRollbackRejected {
        requested_lease_id: u64,
        reason: TasControlRollbackRejectedReason,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasControlCommitted {
        lease_id: u64,
    },
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    TasControlCommitRejected {
        requested_lease_id: u64,
        reason: TasControlCommitRejectedReason,
    },
    SramFlushed(Option<String>),
    RecoveryMissing,
    RecoveryAvailable(crate::save_paths::recovery_state::RecoveryFreshness),
    RecoveryRejected(String),
    RecoverySaved(PathBuf),
    RecoverySaveFailed(String),
    SramFlushFailed(String),
    #[cfg(target_arch = "wasm32")]
    StateBackupRestored(PathBuf),
    #[cfg(target_arch = "wasm32")]
    StateBackupRestoreFailed(String),
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tas_frame_advance_is_a_dedicated_authority_transition() {
        let command = EmuCommand::AdvanceTasControl(Box::new(TasFrameAdvanceRequest {
            profile: TasExecutionProfile::DirectNesCartridge,
            lease_id: 1,
            run_id: 2,
            advance_id: 3,
            segment_id: 4,
            expected_segment_frame_count: 5,
            expected_executed_project_frames: 6,
            expected_frame_count: 4,
            expected_state_sha256: crate::tas_project::TasDigest([5; 32]),
            input: TasInputFrame::default(),
            snapshot: None,
        }));

        assert_eq!(
            command.authority_classification(),
            EmuCommandAuthority::AdvanceTasControl
        );
    }
}
