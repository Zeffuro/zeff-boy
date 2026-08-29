use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use zeff_emu_common::rewind::RewindBuffer;
use zeff_emu_common::time::Reset as _;

use super::ReplayStartState;
use super::recovery::{
    RecoveryCandidate, RecoveryCoordinator, browser_battery_flush_due, should_load_recovery,
};
use super::speculation::SpeculationBoundary;
use super::types::{self, EmuCommand, EmuResponse, FrameInput, FrameResult, SharedFramebuffer};
use super::{DEFAULT_REWIND_SECONDS, REWIND_CAPTURE_INTERVAL_FRAMES, WorkerRuntimeFault};
use crate::cheats::CheatPatch;
use crate::emu_backend::{CoreCapabilities, EmuBackend};
use zeff_emu_common::time::MachineTiming;

const BATTERY_FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const MAX_DEFERRED_STORAGE_COMMANDS: usize = 16;

enum StorageCompletion {
    SaveState {
        path: PathBuf,
        backup_created: bool,
    },
    RestoreStateBackup {
        path: PathBuf,
    },
    Battery {
        epoch: u64,
        snapshot: Vec<crate::platform::SaveWrite>,
        path: Option<String>,
        generation: crate::save_paths::recovery_state::BatteryGenerationRecord,
        recovery_path: Option<PathBuf>,
        shutdown: bool,
    },
}

struct PendingStorage {
    completion: crate::platform::SaveBatchCompletion,
    response: StorageCompletion,
}

struct CapturedBatteryWrites {
    path: Option<String>,
    generation: crate::save_paths::recovery_state::BatteryGenerationRecord,
    recovery_path: Option<PathBuf>,
}

struct Inner {
    backend: EmuBackend,
    pending_frames: VecDeque<FrameResult>,
    pending_responses: VecDeque<EmuResponse>,
    uncapped_mode: bool,
    rewind_buffer: RewindBuffer,
    rewind_seconds: usize,
    frame_duration_ns: u64,
    last_cheats: Vec<CheatPatch>,
    audio_recording_capture: super::AudioRecordingCapture,
    pending_audio_discontinuities: Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    runtime_fault: WorkerRuntimeFault,
    pending_storage: Option<PendingStorage>,
    deferred_storage_commands: VecDeque<EmuCommand>,
    next_battery_flush: web_time::Instant,
    battery_dirty: crate::platform::DirtyEpoch<Vec<crate::platform::SaveWrite>>,
    battery_flush_requested: bool,
    battery_potentially_dirty: bool,
    shutdown_requested: bool,
    recovery: RecoveryCoordinator,
    speculation: SpeculationBoundary,
    save_recovery_on_shutdown: bool,
}

pub(crate) struct EmuThread {
    inner: RefCell<Inner>,
    shared_framebuffer: SharedFramebuffer,
    capabilities: CoreCapabilities,
    audio_recording_context: Option<crate::audio_tooling::AudioRecordingContext>,
}

impl EmuThread {
    pub(crate) fn spawn(backend: EmuBackend, save_recovery_on_shutdown: bool) -> Self {
        let capabilities = backend.capabilities();
        let frame_duration_ns = backend.nominal_frame_duration_ns();
        let audio_recording_context =
            backend
                .audio_topology()
                .map(|topology| crate::audio_tooling::AudioRecordingContext {
                    system: backend.system(),
                    topology,
                    clock_rate: backend.timing_snapshot().rate(),
                });
        let shared_framebuffer = types::new_shared_framebuffer();
        types::publish_framebuffer(&shared_framebuffer, backend.framebuffer());
        let recovery = RecoveryCoordinator::new(&backend);
        Self {
            inner: RefCell::new(Inner {
                backend,
                pending_frames: VecDeque::new(),
                pending_responses: VecDeque::new(),
                uncapped_mode: false,
                rewind_buffer: RewindBuffer::new_with_frame_duration(
                    DEFAULT_REWIND_SECONDS,
                    REWIND_CAPTURE_INTERVAL_FRAMES,
                    frame_duration_ns,
                ),
                rewind_seconds: DEFAULT_REWIND_SECONDS,
                frame_duration_ns,
                last_cheats: Vec::new(),
                audio_recording_capture: super::AudioRecordingCapture::default(),
                pending_audio_discontinuities: Vec::new(),
                runtime_fault: WorkerRuntimeFault::default(),
                pending_storage: None,
                deferred_storage_commands: VecDeque::new(),
                next_battery_flush: web_time::Instant::now() + BATTERY_FLUSH_INTERVAL,
                battery_dirty: crate::platform::DirtyEpoch::default(),
                battery_flush_requested: false,
                battery_potentially_dirty: false,
                shutdown_requested: false,
                recovery,
                speculation: SpeculationBoundary::default(),
                save_recovery_on_shutdown,
            }),
            shared_framebuffer,
            capabilities,
            audio_recording_context,
        }
    }

    pub(crate) fn audio_recording_context(
        &self,
    ) -> Option<crate::audio_tooling::AudioRecordingContext> {
        self.audio_recording_context
    }

    pub(crate) fn capabilities(&self) -> &CoreCapabilities {
        &self.capabilities
    }

    pub(crate) fn nominal_frame_duration_ns(&self) -> u64 {
        self.inner.borrow().frame_duration_ns
    }

    pub(crate) fn shared_framebuffer(&self) -> &SharedFramebuffer {
        &self.shared_framebuffer
    }

    #[cfg(all(test, feature = "wasm-browser-tests"))]
    pub(crate) fn force_detached_frame_for_browser_test(&self) {
        self.inner.borrow_mut().speculation.force_frames_for_test(1);
    }

    #[cfg(all(test, feature = "wasm-browser-tests"))]
    pub(crate) fn speculation_counts_for_browser_test(&self) -> (usize, usize) {
        let inner = self.inner.borrow();
        (
            inner.speculation.completed_runs_for_test(),
            inner.speculation.committed_frames_for_test(),
        )
    }

    #[cfg(all(test, feature = "wasm-browser-tests"))]
    pub(crate) fn primary_framebuffer_for_browser_test(&self) -> Vec<u8> {
        self.inner.borrow().backend.framebuffer().to_vec()
    }

    pub(crate) fn send(&self, cmd: EmuCommand) {
        self.poll_storage();
        self.inner.borrow_mut().speculation.invalidate();
        if Self::is_storage_ordered_command(&cmd) && self.inner.borrow().pending_storage.is_some() {
            let mut inner = self.inner.borrow_mut();
            match cmd {
                EmuCommand::FlushBatterySram => inner.battery_flush_requested = true,
                EmuCommand::Shutdown => {
                    inner.shutdown_requested = true;
                    inner.battery_flush_requested = true;
                }
                command
                    if inner.deferred_storage_commands.len() < MAX_DEFERRED_STORAGE_COMMANDS =>
                {
                    inner.deferred_storage_commands.push_back(command);
                }
                EmuCommand::SaveStateSlot(_) | EmuCommand::SaveStateToPath(_) => inner
                    .pending_responses
                    .push_back(EmuResponse::SaveStateFailed(
                        "browser storage is busy".to_string(),
                    )),
                EmuCommand::RestoreStateBackup(_) => {
                    inner
                        .pending_responses
                        .push_back(EmuResponse::StateBackupRestoreFailed(
                            "browser storage is busy".to_string(),
                        ))
                }
                EmuCommand::LoadStateSlot { .. }
                | EmuCommand::LoadStateFromPath { .. }
                | EmuCommand::InspectRecovery { .. } => {
                    inner
                        .pending_responses
                        .push_back(EmuResponse::LoadStateFailed(
                            "browser storage is busy".to_string(),
                        ))
                }
                _ => unreachable!("non-storage command passed storage ordering gate"),
            }
            return;
        }
        let inner = &mut *self.inner.borrow_mut();
        let Inner {
            backend,
            pending_frames,
            pending_responses,
            uncapped_mode,
            rewind_buffer,
            rewind_seconds,
            frame_duration_ns,
            last_cheats,
            audio_recording_capture,
            pending_audio_discontinuities,
            runtime_fault,
            pending_storage,
            next_battery_flush,
            battery_dirty,
            battery_flush_requested,
            battery_potentially_dirty,
            shutdown_requested,
            recovery,
            speculation,
            save_recovery_on_shutdown,
            deferred_storage_commands: _,
        } = inner;
        match cmd {
            EmuCommand::StepFrames(input) => {
                let input = *input;
                *audio_recording_capture = input.audio.recording_capture;
                let debugger_mutation = !input.debug_actions.memory_writes.is_empty();
                let detached_request = speculation.request_detached_frame(
                    backend,
                    &input,
                    last_cheats,
                    *uncapped_mode,
                    true,
                );
                let mut result = Self::handle_step_frames(
                    backend,
                    input,
                    last_cheats,
                    *uncapped_mode,
                    rewind_buffer,
                    rewind_seconds,
                    runtime_fault,
                );
                let detached_frame = speculation.run_detached_frame(
                    backend,
                    detached_request,
                    &result,
                    runtime_fault.can_step(),
                );
                speculation.commit_primary_frame(
                    &self.shared_framebuffer,
                    backend.framebuffer(),
                    detached_frame,
                );
                if !runtime_fault.can_step() {
                    *uncapped_mode = false;
                }
                if debugger_mutation && runtime_fault.can_step() && audio_recording_capture.semantic
                {
                    pending_audio_discontinuities
                        .push(crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation);
                }
                if !result.audio_semantic_frames.is_empty() {
                    result
                        .audio_timeline_discontinuities
                        .append(pending_audio_discontinuities);
                }
                pending_frames.push_back(result);
                if debugger_mutation
                    || pending_frames
                        .back()
                        .is_some_and(|result| result.advanced_frames > 0)
                {
                    *battery_potentially_dirty = true;
                }
            }
            EmuCommand::SetAudioRecordingCapture {
                capture,
                acknowledged,
            } => {
                if capture.semantic && !audio_recording_capture.semantic {
                    pending_audio_discontinuities.clear();
                }
                *audio_recording_capture = capture;
                if let Some(acknowledged) = acknowledged {
                    let _ = acknowledged.send(());
                }
            }
            EmuCommand::SetSampleRate(rate) => {
                backend.set_sample_rate(rate);
            }
            EmuCommand::SetUncapped(on) => {
                *uncapped_mode = on && runtime_fault.can_step();
                backend.set_apu_sample_generation_enabled(!*uncapped_mode);
            }
            EmuCommand::SetUncappedBatchSize(_) => {}
            EmuCommand::ApplyMediaEvent(event) => {
                let resp = match backend.apply_media_event(&event) {
                    Ok(()) => match backend.media_slot_snapshot() {
                        Some(snapshot) => EmuResponse::MediaEventApplied {
                            event,
                            snapshot,
                            frame_count: backend.frame_count(),
                        },
                        None => EmuResponse::MediaEventFailed {
                            event,
                            error: "media slot disappeared after applying event".to_string(),
                        },
                    },
                    Err(err) => EmuResponse::MediaEventFailed {
                        event,
                        error: err.to_string(),
                    },
                };
                if matches!(&resp, EmuResponse::MediaEventApplied { .. }) {
                    *battery_potentially_dirty = true;
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::SaveStateSlot(slot) => match backend.slot_path(slot) {
                Ok(path) => {
                    Self::begin_save_state(backend, pending_storage, pending_responses, path)
                }
                Err(error) => {
                    pending_responses.push_back(EmuResponse::SaveStateFailed(error.to_string()))
                }
            },
            EmuCommand::LoadStateSlot {
                slot,
                buttons_pressed,
                dpad_pressed,
            } => {
                let resp = Self::load_state_sync(
                    backend,
                    slot,
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                Self::finalize_load_state(
                    &resp,
                    rewind_buffer,
                    *rewind_seconds,
                    frame_duration_ns,
                    backend,
                    last_cheats,
                );
                if matches!(&resp, EmuResponse::LoadStateOk { .. })
                    && audio_recording_capture.semantic
                {
                    pending_audio_discontinuities
                        .push(crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad);
                }
                if matches!(&resp, EmuResponse::LoadStateOk { .. }) {
                    *battery_potentially_dirty = true;
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::SaveStateToPath(path) => {
                Self::begin_save_state(backend, pending_storage, pending_responses, path);
            }
            EmuCommand::LoadStateFromPath {
                path,
                buttons_pressed,
                dpad_pressed,
            } => {
                let result = backend.load_state_from_path(&path);
                let resp = Self::respond_load_state(
                    backend,
                    result,
                    path.display().to_string(),
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                Self::finalize_load_state(
                    &resp,
                    rewind_buffer,
                    *rewind_seconds,
                    frame_duration_ns,
                    backend,
                    last_cheats,
                );
                if matches!(&resp, EmuResponse::LoadStateOk { .. })
                    && audio_recording_capture.semantic
                {
                    pending_audio_discontinuities
                        .push(crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad);
                }
                if matches!(&resp, EmuResponse::LoadStateOk { .. }) {
                    *battery_potentially_dirty = true;
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::InspectRecovery {
                resume,
                buttons_pressed,
                dpad_pressed,
            } => {
                let resp = match recovery.inspect(backend) {
                    RecoveryCandidate::Missing => EmuResponse::RecoveryMissing,
                    RecoveryCandidate::Rejected(error) => EmuResponse::RecoveryRejected(error),
                    RecoveryCandidate::Available {
                        freshness,
                        native_payload,
                        path,
                    } if should_load_recovery(freshness, resume) => {
                        let result = backend.load_state_from_bytes(native_payload);
                        Self::respond_load_state(
                            backend,
                            result,
                            path.display().to_string(),
                            buttons_pressed,
                            dpad_pressed,
                            &self.shared_framebuffer,
                        )
                    }
                    RecoveryCandidate::Available { freshness, .. } => {
                        EmuResponse::RecoveryAvailable(freshness)
                    }
                };
                Self::finalize_load_state(
                    &resp,
                    rewind_buffer,
                    *rewind_seconds,
                    frame_duration_ns,
                    backend,
                    last_cheats,
                );
                if matches!(&resp, EmuResponse::LoadStateOk { .. })
                    && audio_recording_capture.semantic
                {
                    pending_audio_discontinuities
                        .push(crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad);
                }
                if matches!(&resp, EmuResponse::LoadStateOk { .. }) {
                    *battery_potentially_dirty = true;
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::CaptureStateBytes => {
                let resp = match backend.encode_state_bytes() {
                    Ok(bytes) => EmuResponse::StateCaptured(bytes),
                    Err(e) => EmuResponse::StateCaptureFailed(e.to_string()),
                };
                pending_responses.push_back(resp);
            }
            EmuCommand::ExecuteGuestCall(request) => {
                let name = request.name.clone();
                let resp = match backend.execute_guest_call(&request) {
                    Ok((instructions, undo_state)) => EmuResponse::GuestCallCompleted {
                        name,
                        instructions,
                        undo_state,
                    },
                    Err(error) => EmuResponse::GuestCallFailed {
                        name,
                        error: error.to_string(),
                    },
                };
                types::publish_framebuffer(&self.shared_framebuffer, backend.framebuffer());
                if matches!(&resp, EmuResponse::GuestCallCompleted { .. })
                    && audio_recording_capture.semantic
                {
                    pending_audio_discontinuities
                        .push(crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation);
                }
                if matches!(&resp, EmuResponse::GuestCallCompleted { .. }) {
                    *battery_potentially_dirty = true;
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::UndoGuestCall(state) => {
                let resp = if backend.supports_guest_calls() {
                    match backend.load_state_from_bytes(state) {
                        Ok(()) => EmuResponse::GuestCallUndone,
                        Err(error) => EmuResponse::GuestCallUndoFailed(error.to_string()),
                    }
                } else {
                    EmuResponse::GuestCallUndoFailed(
                        "guest calls are not supported by this core".to_string(),
                    )
                };
                types::publish_framebuffer(&self.shared_framebuffer, backend.framebuffer());
                if matches!(&resp, EmuResponse::GuestCallUndone) && audio_recording_capture.semantic
                {
                    pending_audio_discontinuities
                        .push(crate::audio_recorder::AudioTimelineDiscontinuity::GuestCallUndo);
                }
                if matches!(&resp, EmuResponse::GuestCallUndone) {
                    backend.discard_game_boy_printer_jobs();
                    *battery_potentially_dirty = true;
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::CaptureReplayStart { capture_id } => {
                let resp = if !backend.supports_replay() {
                    EmuResponse::ReplayStartCaptureFailed {
                        capture_id,
                        error: "replay capture is not supported by this core".to_string(),
                    }
                } else {
                    match backend.encode_replay_start_state_bytes() {
                        Ok(bytes) => EmuResponse::ReplayStartCaptured {
                            capture_id,
                            start: Box::new(ReplayStartState {
                                state_bytes: bytes,
                                frame_count: backend.frame_count(),
                                game_boy_cpu_cycles: backend.game_boy_cpu_cycles(),
                                wonder_swan_cpu_cycles: backend.wonder_swan_cpu_cycles(),
                                metadata: backend.replay_metadata(),
                            }),
                        },
                        Err(e) => EmuResponse::ReplayStartCaptureFailed {
                            capture_id,
                            error: e.to_string(),
                        },
                    }
                };
                pending_responses.push_back(resp);
            }
            EmuCommand::CaptureReplayCheckpoint { frame } => {
                let resp = if backend.supports_replay() {
                    match backend.encode_state_bytes() {
                        Ok(state_bytes) => {
                            EmuResponse::ReplayCheckpointCaptured { frame, state_bytes }
                        }
                        Err(error) => EmuResponse::ReplayCheckpointCaptureFailed {
                            frame,
                            error: error.to_string(),
                        },
                    }
                } else {
                    EmuResponse::ReplayCheckpointCaptureFailed {
                        frame,
                        error: "replay capture is not supported by this core".to_string(),
                    }
                };
                pending_responses.push_back(resp);
            }
            EmuCommand::LoadStateBytes {
                state_bytes,
                buttons_pressed,
                dpad_pressed,
                replay_events: _,
                game_boy_link_start_state: _,
                game_boy_link_coordinator_start_state: _,
                game_boy_link_start_tick: _,
                wonder_swan_link_start_tick: _,
            } => {
                let result = backend.load_state_from_bytes(state_bytes);
                let resp = Self::respond_load_state(
                    backend,
                    result,
                    "bytes".to_string(),
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                Self::finalize_load_state(
                    &resp,
                    rewind_buffer,
                    *rewind_seconds,
                    frame_duration_ns,
                    backend,
                    last_cheats,
                );
                if matches!(&resp, EmuResponse::LoadStateOk { .. })
                    && audio_recording_capture.semantic
                {
                    pending_audio_discontinuities
                        .push(crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad);
                }
                if matches!(&resp, EmuResponse::LoadStateOk { .. }) {
                    *battery_potentially_dirty = true;
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::SetGameBoySerialDevice(device) => {
                backend.set_game_boy_serial_device(device);
            }
            EmuCommand::QueueBardigunBarcodeScan(bytes) => {
                let byte_count = bytes.len();
                let response = match backend.queue_bardigun_barcode_scan(bytes) {
                    Ok(()) => EmuResponse::BardigunBarcodeScanStarted(byte_count),
                    Err(err) => EmuResponse::BardigunBarcodeScanFailed(err.to_string()),
                };
                pending_responses.push_back(response);
            }
            EmuCommand::TriggerBarcodeBoyScan(digits) => {
                let response = match backend.trigger_barcode_boy_scan(&digits) {
                    Ok(()) => EmuResponse::BarcodeBoyScanStarted,
                    Err(err) => EmuResponse::BarcodeBoyScanFailed(err.to_string()),
                };
                pending_responses.push_back(response);
            }
            EmuCommand::RestoreGameBoyLinkState(state) => {
                backend.restore_game_boy_link_replay_state(state);
            }
            EmuCommand::UpdateCheats(patches) => {
                if backend.supports_cheats() {
                    *last_cheats = patches;
                    backend.install_rom_patches(last_cheats);
                } else {
                    last_cheats.clear();
                }
            }
            EmuCommand::Rewind(steps) => {
                let resp =
                    Self::handle_rewind(backend, rewind_buffer, &self.shared_framebuffer, steps);
                if matches!(&resp, EmuResponse::RewindOk { .. }) {
                    *battery_potentially_dirty = true;
                    backend.discard_game_boy_printer_jobs();
                    backend.install_rom_patches(last_cheats);
                    if audio_recording_capture.semantic {
                        pending_audio_discontinuities
                            .push(crate::audio_recorder::AudioTimelineDiscontinuity::Rewind);
                    }
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::Reset => {
                backend.reset();
                *battery_potentially_dirty = true;
                backend.install_rom_patches(last_cheats);
                rewind_buffer.clear();
                *runtime_fault = WorkerRuntimeFault::default();
                if audio_recording_capture.semantic {
                    pending_audio_discontinuities
                        .push(crate::audio_recorder::AudioTimelineDiscontinuity::Reset);
                }
            }
            EmuCommand::FlushBatterySram => Self::request_battery_flush(
                backend,
                recovery,
                *save_recovery_on_shutdown,
                pending_storage,
                pending_responses,
                next_battery_flush,
                battery_dirty,
                battery_flush_requested,
                battery_potentially_dirty,
                shutdown_requested,
                speculation,
                false,
            ),
            EmuCommand::RestoreStateBackup(path) => {
                Self::begin_restore_state_backup(pending_storage, pending_responses, path)
            }
            EmuCommand::Shutdown => Self::request_battery_flush(
                backend,
                recovery,
                *save_recovery_on_shutdown,
                pending_storage,
                pending_responses,
                next_battery_flush,
                battery_dirty,
                battery_flush_requested,
                battery_potentially_dirty,
                shutdown_requested,
                speculation,
                true,
            ),
        }
    }

    pub(crate) fn try_recv_frame(&self) -> Option<FrameResult> {
        self.poll_storage();
        self.inner.borrow_mut().pending_frames.pop_front()
    }

    pub(crate) fn recv(&self) -> Option<EmuResponse> {
        self.poll_storage();
        self.inner.borrow_mut().pending_responses.pop_front()
    }

    pub(crate) fn try_recv_response(&self) -> Option<EmuResponse> {
        self.poll_storage();
        self.inner.borrow_mut().pending_responses.pop_front()
    }

    pub(crate) fn shutdown(&mut self) {
        self.send(EmuCommand::Shutdown);
    }

    pub(crate) fn poll_persistence(&self) {
        self.poll_storage();
    }

    fn is_storage_ordered_command(command: &EmuCommand) -> bool {
        matches!(
            command,
            EmuCommand::SaveStateSlot(_)
                | EmuCommand::LoadStateSlot { .. }
                | EmuCommand::SaveStateToPath(_)
                | EmuCommand::LoadStateFromPath { .. }
                | EmuCommand::InspectRecovery { .. }
                | EmuCommand::FlushBatterySram
                | EmuCommand::RestoreStateBackup(_)
                | EmuCommand::Shutdown
        )
    }

    fn begin_save_state(
        backend: &EmuBackend,
        pending_storage: &mut Option<PendingStorage>,
        pending_responses: &mut VecDeque<EmuResponse>,
        path: PathBuf,
    ) {
        let captured = crate::platform::capture_save_writes(|| {
            let bytes = backend.encode_state_bytes()?;
            crate::save_paths::write_state_bytes_to_file_with_backup(&path, &bytes)
        });
        let (backup_created, writes) = match captured {
            Ok(captured) => captured,
            Err(error) => {
                pending_responses.push_back(EmuResponse::SaveStateFailed(error.to_string()));
                return;
            }
        };
        let completion = Rc::new(RefCell::new(None));
        crate::platform::commit_save_writes(writes, completion.clone());
        *pending_storage = Some(PendingStorage {
            completion,
            response: StorageCompletion::SaveState {
                path,
                backup_created,
            },
        });
    }

    fn begin_restore_state_backup(
        pending_storage: &mut Option<PendingStorage>,
        pending_responses: &mut VecDeque<EmuResponse>,
        path: PathBuf,
    ) {
        let captured = crate::platform::capture_save_writes(|| {
            crate::save_paths::restore_state_file_backup(&path)
        });
        let (_, writes) = match captured {
            Ok(captured) => captured,
            Err(error) => {
                pending_responses
                    .push_back(EmuResponse::StateBackupRestoreFailed(error.to_string()));
                return;
            }
        };
        let completion = Rc::new(RefCell::new(None));
        crate::platform::commit_save_writes(writes, completion.clone());
        *pending_storage = Some(PendingStorage {
            completion,
            response: StorageCompletion::RestoreStateBackup { path },
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn request_battery_flush(
        backend: &mut EmuBackend,
        recovery: &mut RecoveryCoordinator,
        save_recovery_on_shutdown: bool,
        pending_storage: &mut Option<PendingStorage>,
        pending_responses: &mut VecDeque<EmuResponse>,
        next_battery_flush: &mut web_time::Instant,
        battery_dirty: &mut crate::platform::DirtyEpoch<Vec<crate::platform::SaveWrite>>,
        battery_flush_requested: &mut bool,
        battery_potentially_dirty: &mut bool,
        shutdown_requested: &mut bool,
        speculation: &mut SpeculationBoundary,
        shutdown: bool,
    ) {
        *shutdown_requested |= shutdown;
        let terminal = *shutdown_requested;
        let terminal_ready = terminal.then(|| speculation.prepare_terminal_persistence());
        if pending_storage.is_some() {
            *battery_flush_requested = true;
            return;
        }
        *battery_flush_requested = false;
        *next_battery_flush = web_time::Instant::now() + BATTERY_FLUSH_INTERVAL;
        let captured = match terminal_ready {
            Some(ready) => Self::capture_terminal_save_writes(
                ready,
                backend,
                recovery,
                save_recovery_on_shutdown,
            ),
            None => Self::capture_battery_save_writes(backend, recovery, false),
        };
        let (captured, snapshot) = match captured {
            Ok(captured) => captured,
            Err(error) => {
                pending_responses.push_back(EmuResponse::SramFlushFailed(error.to_string()));
                if terminal && save_recovery_on_shutdown {
                    pending_responses.push_back(EmuResponse::RecoverySaveFailed(
                        "browser recovery transaction could not be prepared".to_string(),
                    ));
                }
                if std::mem::take(shutdown_requested) {
                    pending_responses.push_back(EmuResponse::ShutdownComplete);
                }
                return;
            }
        };
        let CapturedBatteryWrites {
            path,
            generation,
            recovery_path,
        } = captured;
        let epoch = battery_dirty.observe(&snapshot);
        if crate::platform::save_writes_are_committed(&snapshot) {
            backend.acknowledge_battery_commit(true);
            recovery.acknowledge_generation(generation);
            *battery_potentially_dirty = false;
            pending_responses.push_back(EmuResponse::SramFlushed(None));
            if let Some(path) = recovery_path {
                pending_responses.push_back(EmuResponse::RecoverySaved(path));
            }
            if std::mem::take(shutdown_requested) {
                pending_responses.push_back(EmuResponse::ShutdownComplete);
            }
            return;
        }

        let completion = Rc::new(RefCell::new(None));
        crate::platform::commit_save_writes(snapshot.clone(), completion.clone());
        *pending_storage = Some(PendingStorage {
            completion,
            response: StorageCompletion::Battery {
                epoch,
                snapshot,
                path,
                generation,
                recovery_path,
                shutdown: terminal,
            },
        });
    }

    fn capture_terminal_save_writes(
        _ready: super::speculation::TerminalPersistenceReady,
        backend: &mut EmuBackend,
        recovery: &RecoveryCoordinator,
        save_recovery_on_shutdown: bool,
    ) -> anyhow::Result<(CapturedBatteryWrites, Vec<crate::platform::SaveWrite>)> {
        let include_recovery = save_recovery_on_shutdown && backend.supports_save_states();
        Self::capture_battery_save_writes(backend, recovery, include_recovery)
    }

    fn capture_battery_save_writes(
        backend: &mut EmuBackend,
        recovery: &RecoveryCoordinator,
        include_recovery: bool,
    ) -> anyhow::Result<(CapturedBatteryWrites, Vec<crate::platform::SaveWrite>)> {
        crate::platform::capture_save_writes(|| {
            let path = backend.flush_battery_sram()?;
            let generation = recovery.capture_generation_write(backend)?;
            let recovery_path = include_recovery
                .then(|| recovery.encode_and_capture_recovery_write(backend, generation))
                .transpose()?;
            Ok(CapturedBatteryWrites {
                path,
                generation,
                recovery_path,
            })
        })
    }

    fn poll_storage(&self) {
        let mut next_command = None;
        {
            let mut inner = self.inner.borrow_mut();
            let completed = inner.pending_storage.as_ref().and_then(|pending| {
                pending
                    .completion
                    .borrow_mut()
                    .take()
                    .map(|result| (result, ()))
            });
            if let Some((result, ())) = completed {
                let pending = inner
                    .pending_storage
                    .take()
                    .expect("pending storage disappeared");
                Self::finish_storage(&mut inner, pending.response, result);
            }

            let browser_flush_due = browser_battery_flush_due(
                inner.battery_flush_requested,
                inner.battery_potentially_dirty,
                web_time::Instant::now() >= inner.next_battery_flush,
            );
            if inner.pending_storage.is_none()
                && inner.deferred_storage_commands.is_empty()
                && browser_flush_due
            {
                let Inner {
                    backend,
                    pending_responses,
                    pending_storage,
                    next_battery_flush,
                    battery_dirty,
                    battery_flush_requested,
                    battery_potentially_dirty,
                    shutdown_requested,
                    recovery,
                    save_recovery_on_shutdown,
                    speculation,
                    ..
                } = &mut *inner;
                Self::request_battery_flush(
                    backend,
                    recovery,
                    *save_recovery_on_shutdown,
                    pending_storage,
                    pending_responses,
                    next_battery_flush,
                    battery_dirty,
                    battery_flush_requested,
                    battery_potentially_dirty,
                    shutdown_requested,
                    speculation,
                    false,
                );
            }
            if inner.pending_storage.is_none() {
                next_command = inner.deferred_storage_commands.pop_front();
            }
        }
        if let Some(command) = next_command {
            self.send(command);
        }
    }

    fn finish_storage(
        inner: &mut Inner,
        completion: StorageCompletion,
        result: Result<(), String>,
    ) {
        match completion {
            StorageCompletion::SaveState {
                path,
                backup_created,
            } => match result {
                Ok(()) => inner.pending_responses.push_back(EmuResponse::SaveStateOk {
                    path,
                    backup_created,
                }),
                Err(error) => inner
                    .pending_responses
                    .push_back(EmuResponse::SaveStateFailed(error)),
            },
            StorageCompletion::RestoreStateBackup { path } => match result {
                Ok(()) => inner
                    .pending_responses
                    .push_back(EmuResponse::StateBackupRestored(path)),
                Err(error) => inner
                    .pending_responses
                    .push_back(EmuResponse::StateBackupRestoreFailed(error)),
            },
            StorageCompletion::Battery {
                epoch,
                snapshot,
                path,
                generation,
                recovery_path,
                shutdown,
            } => match result {
                Ok(()) => {
                    let still_current = inner.battery_dirty.acknowledges(epoch, &snapshot)
                        && inner.backend.battery_component_hash() == generation.component_sha256
                        && crate::platform::save_writes_are_committed(&snapshot);
                    if still_current {
                        inner.backend.acknowledge_battery_commit(true);
                        inner.recovery.acknowledge_generation(generation);
                        inner.battery_potentially_dirty = false;
                        inner
                            .pending_responses
                            .push_back(EmuResponse::SramFlushed(path));
                        if let Some(path) = recovery_path {
                            inner
                                .pending_responses
                                .push_back(EmuResponse::RecoverySaved(path));
                        }
                        if shutdown || inner.shutdown_requested {
                            inner.shutdown_requested = false;
                            inner
                                .pending_responses
                                .push_back(EmuResponse::ShutdownComplete);
                        }
                    } else {
                        inner.backend.acknowledge_battery_commit(false);
                        inner.battery_potentially_dirty = true;
                        inner.battery_flush_requested = true;
                    }
                }
                Err(error) => {
                    inner.battery_potentially_dirty = true;
                    inner
                        .pending_responses
                        .push_back(EmuResponse::SramFlushFailed(error));
                    if shutdown && inner.save_recovery_on_shutdown {
                        inner
                            .pending_responses
                            .push_back(EmuResponse::RecoverySaveFailed(
                                "browser recovery transaction did not commit".to_string(),
                            ));
                    }
                    if shutdown || inner.shutdown_requested {
                        inner.shutdown_requested = false;
                        inner
                            .pending_responses
                            .push_back(EmuResponse::ShutdownComplete);
                    }
                }
            },
        }
    }

    fn load_state_sync(
        backend: &mut EmuBackend,
        slot: u8,
        buttons_pressed: u8,
        dpad_pressed: u8,
        shared_fb: &SharedFramebuffer,
    ) -> EmuResponse {
        let path = match backend.slot_path(slot) {
            Ok(p) => p,
            Err(e) => return EmuResponse::LoadStateFailed(e.to_string()),
        };
        let result = backend.load_state_from_path(&path);
        Self::respond_load_state(
            backend,
            result,
            path.display().to_string(),
            buttons_pressed,
            dpad_pressed,
            shared_fb,
        )
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use crate::audio_tooling::AudioChannelId;
    use crate::emu_thread::{
        AudioConfig, AudioRecordingCapture, JoypadInput, MemorySearchRequest, RenderSettings,
        ReusableBuffers, SnapshotRequest, SpeculationBlockers, ZapperInput,
    };
    use wasm_bindgen_test::wasm_bindgen_test;

    #[cfg(feature = "wasm-browser-tests")]
    #[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function browser_test_delay(milliseconds) {
    return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
")]
    extern "C" {
        #[wasm_bindgen::prelude::wasm_bindgen(catch)]
        async fn browser_test_delay(
            milliseconds: u32,
        ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>;
    }

    struct PrimaryObservation {
        state: Vec<u8>,
        framebuffer: Vec<u8>,
        residual_audio: Vec<f32>,
        battery_hash: [u8; 32],
        battery_bytes: Option<Vec<u8>>,
        gba_rtc: Option<zeff_gba_core::hardware::cartridge::RtcDateTime>,
        potentially_dirty: bool,
    }

    fn sms_thread() -> EmuThread {
        sms_thread_with_recovery(false)
    }

    fn sms_thread_with_recovery(save_recovery_on_shutdown: bool) -> EmuThread {
        const ROM: &[u8] = &[
            0x3E, 0x80, 0xD3, 0x7F, // Tone 0 low period.
            0x3E, 0x04, 0xD3, 0x7F, // Tone 0 high period.
            0x3E, 0x90, 0xD3, 0x7F, // Tone 0 full volume.
            0x76, // Halt while the PSG continues.
        ];
        let mut emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
            ROM,
            44_100,
            zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        )
        .unwrap();
        emu.set_instruction_trace_enabled(true);
        let backend = EmuBackend::from_sega8(emu, PathBuf::from("wasm-test.sms"));
        assert!(!backend.save_ram_kind().is_battery_backed());
        EmuThread::spawn(backend, save_recovery_on_shutdown)
    }

    fn gba_test_rom() -> Vec<u8> {
        let mut rom = vec![0; 0xC0];
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xAC..0xB0].copy_from_slice(b"BPEE");
        rom[0xB0..0xB2].copy_from_slice(b"01");
        rom[0xB2] = 0x96;
        rom.extend_from_slice(b"SRAM_V113");
        rom
    }

    fn gba_rtc() -> zeff_gba_core::hardware::cartridge::RtcDateTime {
        zeff_gba_core::hardware::cartridge::RtcDateTime::new(2032, 9, 17, 5, [21, 43, 19]).unwrap()
    }

    fn gba_sram_bytes(len: usize) -> Vec<u8> {
        (0..len)
            .map(|index| (index as u8).wrapping_mul(17).wrapping_add(3))
            .collect()
    }

    fn gba_thread(save_recovery_on_shutdown: bool) -> EmuThread {
        let mut emu = zeff_gba_core::emulator::Emulator::new(&gba_test_rom(), 44_100).unwrap();
        let sram = gba_sram_bytes(emu.dump_battery_sram().unwrap().len());
        emu.load_battery_sram(&sram).unwrap();
        assert!(emu.set_rtc_date_time(gba_rtc()));
        emu.set_instruction_trace_enabled(true);
        let backend = EmuBackend::from_gba(emu, PathBuf::from("wasm-emerald-sram.gba"));
        assert!(backend.save_ram_kind().is_battery_backed());
        EmuThread::spawn(backend, save_recovery_on_shutdown)
    }

    fn frame_input() -> FrameInput {
        FrameInput {
            frames: 1,
            speculation_blockers: SpeculationBlockers::from_app_for_test(false, false),
            replay_joypad_frames: None,
            host_tilt: (0.0, 0.0),
            host_camera_frame: None,
            joypad: JoypadInput {
                buttons: 0x01,
                dpad: 0x02,
                buttons_p2: 0,
                dpad_p2: 0,
                buttons_p3: 0,
                dpad_p3: 0,
                buttons_p4: 0,
                dpad_p4: 0,
                buttons_p5: 0,
                dpad_p5: 0,
            },
            pce_mouse: Default::default(),
            zapper: ZapperInput::default(),
            debug_step: false,
            debug_continue: false,
            debug_suspend_after_frame: false,
            audio: AudioConfig {
                apu_capture_enabled: true,
                skip_audio: false,
                playback_speed: 1,
                recording_capture: AudioRecordingCapture {
                    active: true,
                    semantic: true,
                },
            },
            debug_actions: crate::debug::DebugUiActions::none(),
            snapshot: SnapshotRequest {
                want_debug_info: true,
                want_perf_info: true,
                any_viewer_open: true,
                any_vram_viewer_open: true,
                show_oam_viewer: true,
                show_apu_viewer: true,
                show_disassembler: true,
                show_rom_info: true,
                show_memory_viewer: true,
                memory_view_start: 0,
                show_rom_viewer: true,
                show_instruction_trace: true,
                trace_after_sequence: None,
                rom_view_start: 0,
                last_disasm_pc: None,
                last_disasm_mapping: None,
                disasm_target: None,
                memory_search: Some(MemorySearchRequest {
                    pattern: vec![0x3E, 0x80, 0xD3, 0x7F],
                    max_results: 4,
                }),
                rom_search: Some(MemorySearchRequest {
                    pattern: vec![0x3E, 0x80, 0xD3, 0x7F],
                    max_results: 4,
                }),
                render: RenderSettings {
                    color_correction: crate::settings::ColorCorrection::None,
                    color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                    dmg_palette_preset: crate::settings::DmgPalettePreset::default(),
                    nes_palette_mode: crate::settings::NesPaletteMode::default(),
                    nes_custom_palette: None,
                    pce_overscan_mode: crate::settings::PceOverscanMode::default(),
                    pce_palette_mode: crate::settings::PcePaletteMode::default(),
                    sgb_border_enabled: false,
                },
            },
            buffers: ReusableBuffers {
                audio: None,
                vram: None,
                oam: None,
                memory_page: None,
                nes_chr: None,
                nes_nametable: None,
            },
            rewind_enabled: false,
            rewind_seconds: 10,
        }
    }

    fn gba_frame_input() -> FrameInput {
        let mut input = frame_input();
        input.snapshot.memory_view_start = 0x0200_0000;
        input.snapshot.memory_search = Some(MemorySearchRequest {
            pattern: vec![0, 0, 0, 0],
            max_results: 4,
        });
        input.snapshot.rom_search = Some(MemorySearchRequest {
            pattern: b"BPEE".to_vec(),
            max_results: 4,
        });
        input
    }

    fn observe_primary(thread: &EmuThread) -> PrimaryObservation {
        let mut inner = thread.inner.borrow_mut();
        let mut residual_audio = Vec::new();
        inner.backend.drain_audio_samples_into(&mut residual_audio);
        let (battery_bytes, gba_rtc) = match &inner.backend {
            EmuBackend::Gba(backend) => {
                (backend.emu.dump_battery_sram(), backend.emu.rtc_date_time())
            }
            _ => (None, None),
        };
        PrimaryObservation {
            state: inner.backend.encode_state_bytes().unwrap(),
            framebuffer: inner.backend.framebuffer().to_vec(),
            residual_audio,
            battery_hash: inner.backend.battery_component_hash(),
            battery_bytes,
            gba_rtc,
            potentially_dirty: inner.battery_potentially_dirty,
        }
    }

    fn assert_primary_matches(left: &PrimaryObservation, right: &PrimaryObservation) {
        assert_eq!(left.state, right.state);
        assert_eq!(left.framebuffer, right.framebuffer);
        assert_eq!(left.residual_audio, right.residual_audio);
        assert!(left.residual_audio.is_empty());
        assert_eq!(left.battery_hash, right.battery_hash);
        assert_eq!(left.battery_bytes, right.battery_bytes);
        assert_eq!(left.gba_rtc, right.gba_rtc);
        assert_eq!(left.potentially_dirty, right.potentially_dirty);
    }

    fn assert_frame_results_match(left: &FrameResult, right: &FrameResult) {
        assert_eq!(left.advanced_frames, right.advanced_frames);
        assert_eq!(left.delivery_merged, right.delivery_merged);
        assert_eq!(left.replay_events, right.replay_events);
        assert_eq!(left.replay_error, right.replay_error);
        assert_eq!(left.runtime_fault, right.runtime_fault);
        assert_eq!(left.rumble, right.rumble);
        assert_f32_equal("PCM", &left.audio_samples, &right.audio_samples);
        assert_eq!(left.audio_playback_speed, right.audio_playback_speed);
        assert_eq!(left.is_mbc7, right.is_mbc7);
        assert_eq!(left.is_pocket_camera, right.is_pocket_camera);
        assert_eq!(left.game_boy_serial_device, right.game_boy_serial_device);
        assert_eq!(
            left.game_boy_printer_jobs.len(),
            right.game_boy_printer_jobs.len()
        );
        assert_eq!(left.media_slot_snapshot, right.media_slot_snapshot);
        assert_eq!(left.rewind_fill.to_bits(), right.rewind_fill.to_bits());
        assert_eq!(left.audio_semantic_frames, right.audio_semantic_frames);
        assert_eq!(
            left.audio_timeline_discontinuities,
            right.audio_timeline_discontinuities
        );
        assert_eq!(left.ui_data.core_features, right.ui_data.core_features);
        assert_eq!(
            left.ui_data.cpu_debug.is_some(),
            right.ui_data.cpu_debug.is_some()
        );
        assert_eq!(
            left.ui_data.perf_info.is_some(),
            right.ui_data.perf_info.is_some()
        );
        assert_eq!(
            left.ui_data.apu_debug.is_some(),
            right.ui_data.apu_debug.is_some()
        );
        assert_eq!(
            left.ui_data.oam_debug.is_some(),
            right.ui_data.oam_debug.is_some()
        );
        assert_eq!(
            left.ui_data.palette_debug.is_some(),
            right.ui_data.palette_debug.is_some()
        );
        assert_eq!(
            left.ui_data.rom_debug.is_some(),
            right.ui_data.rom_debug.is_some()
        );
        assert_eq!(
            left.ui_data.input_debug.is_some(),
            right.ui_data.input_debug.is_some()
        );
        assert_eq!(
            left.ui_data.graphics_data.is_some(),
            right.ui_data.graphics_data.is_some()
        );
        assert_eq!(
            left.ui_data.disassembly_view.is_some(),
            right.ui_data.disassembly_view.is_some()
        );
        assert_eq!(left.ui_data.memory_page, right.ui_data.memory_page);
        assert_eq!(
            left.ui_data.memory_search_results.is_some(),
            right.ui_data.memory_search_results.is_some()
        );
        assert_eq!(left.ui_data.rom_page, right.ui_data.rom_page);
        assert_eq!(left.ui_data.rom_size, right.ui_data.rom_size);
        assert_eq!(
            left.ui_data.rom_search_results.is_some(),
            right.ui_data.rom_search_results.is_some()
        );
        assert_eq!(
            left.ui_data.instruction_trace.is_some(),
            right.ui_data.instruction_trace.is_some()
        );
    }

    fn assert_bytes_equal(label: &str, left: &[u8], right: &[u8]) {
        assert_eq!(left.len(), right.len(), "{label} length");
        assert!(
            left == right,
            "{label} content differs: left={:016X} right={:016X}",
            byte_signature(left),
            byte_signature(right)
        );
    }

    fn byte_signature(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01B3)
        })
    }

    fn assert_f32_equal(label: &str, left: &[f32], right: &[f32]) {
        assert_eq!(left.len(), right.len(), "{label} length");
        let exact = left
            .iter()
            .zip(right)
            .all(|(left, right)| left.to_bits() == right.to_bits());
        assert!(
            exact,
            "{label} content differs: left={:016X} right={:016X}",
            f32_signature(left),
            f32_signature(right)
        );
    }

    fn f32_signature(samples: &[f32]) -> u64 {
        samples.iter().fold(0xcbf2_9ce4_8422_2325, |hash, sample| {
            (hash ^ u64::from(sample.to_bits())).wrapping_mul(0x0000_0100_0000_01B3)
        })
    }

    fn assert_rich_ui_data_match(left: &FrameResult, right: &FrameResult) {
        assert!(left.ui_data.core_features.is_some());
        let left_cpu = left.ui_data.cpu_debug.as_ref().expect("CPU debug data");
        let right_cpu = right.ui_data.cpu_debug.as_ref().expect("CPU debug data");
        assert_eq!(left_cpu.register_lines, right_cpu.register_lines);
        assert!(!left_cpu.register_lines.is_empty());
        assert_eq!(left_cpu.flags, right_cpu.flags);
        assert_eq!(left_cpu.status_text, right_cpu.status_text);
        assert_eq!(left_cpu.cpu_state, right_cpu.cpu_state);
        assert_eq!(left_cpu.pc, right_cpu.pc);
        assert_eq!(left_cpu.cycles, right_cpu.cycles);
        assert_eq!(left_cpu.last_opcode_line, right_cpu.last_opcode_line);
        assert_eq!(left_cpu.sections, right_cpu.sections);
        assert_eq!(left_cpu.io_registers, right_cpu.io_registers);
        assert_eq!(
            left_cpu.recent_opcodes.len(),
            right_cpu.recent_opcodes.len()
        );
        assert!(!left_cpu.recent_opcodes.is_empty());
        for (left_op, right_op) in left_cpu
            .recent_opcodes
            .iter()
            .zip(&right_cpu.recent_opcodes)
        {
            assert_eq!(left_op.address, right_op.address);
            assert_eq!(left_op.storage_offset, right_op.storage_offset);
            assert_bytes_equal("recent opcode", &left_op.bytes, &right_op.bytes);
            assert_eq!(left_op.detail, right_op.detail);
            assert_eq!(left_op.repeat_count, right_op.repeat_count);
            assert_eq!(left_op.thumb, right_op.thumb);
        }
        assert_eq!(left_cpu.call_stack.len(), right_cpu.call_stack.len());
        for (left_call, right_call) in left_cpu.call_stack.iter().zip(&right_cpu.call_stack) {
            assert_eq!(left_call.target, right_call.target);
            assert_eq!(left_call.return_address, right_call.return_address);
            assert_eq!(left_call.target_rom_offset, right_call.target_rom_offset);
            assert_eq!(left_call.return_rom_offset, right_call.return_rom_offset);
            assert_eq!(left_call.kind, right_call.kind);
        }
        assert_eq!(
            left_cpu.call_stack_available,
            right_cpu.call_stack_available
        );
        assert_eq!(left_cpu.breakpoints, right_cpu.breakpoints);
        assert_eq!(
            left_cpu.one_shot_breakpoints,
            right_cpu.one_shot_breakpoints
        );
        assert_eq!(
            left_cpu.breakpoint_hit_conditions,
            right_cpu.breakpoint_hit_conditions
        );
        assert_eq!(left_cpu.supported_events, right_cpu.supported_events);
        assert_eq!(left_cpu.event_breakpoints, right_cpu.event_breakpoints);
        assert_eq!(left_cpu.rom_breakpoints, right_cpu.rom_breakpoints);
        assert_eq!(left_cpu.watchpoints.len(), right_cpu.watchpoints.len());
        for (left_watch, right_watch) in left_cpu.watchpoints.iter().zip(&right_cpu.watchpoints) {
            assert_eq!(left_watch.address, right_watch.address);
            assert_eq!(left_watch.end_address, right_watch.end_address);
            assert_eq!(left_watch.watch_type, right_watch.watch_type);
        }
        assert_eq!(left_cpu.hit_breakpoint, right_cpu.hit_breakpoint);
        assert_eq!(left_cpu.hit_rom_breakpoint, right_cpu.hit_rom_breakpoint);
        match (&left_cpu.hit_watchpoint, &right_cpu.hit_watchpoint) {
            (None, None) => {}
            (Some(left_hit), Some(right_hit)) => {
                assert_eq!(left_hit.address, right_hit.address);
                assert_eq!(left_hit.old_value, right_hit.old_value);
                assert_eq!(left_hit.new_value, right_hit.new_value);
                assert_eq!(left_hit.watch_type, right_hit.watch_type);
            }
            _ => panic!("watchpoint hit presence differs"),
        }
        assert_eq!(left_cpu.hit_event, right_cpu.hit_event);

        let left_apu = left.ui_data.apu_debug.as_ref().expect("APU data");
        let right_apu = right.ui_data.apu_debug.as_ref().expect("APU data");
        assert_eq!(left_apu.extra_sections, right_apu.extra_sections);

        let left_perf = left.ui_data.perf_info.as_ref().expect("performance data");
        let right_perf = right.ui_data.perf_info.as_ref().expect("performance data");
        assert_eq!(left_perf.fps.to_bits(), right_perf.fps.to_bits());
        assert_eq!(
            left_perf.target_fps.to_bits(),
            right_perf.target_fps.to_bits()
        );
        assert_eq!(left_perf.speed_mode_label, right_perf.speed_mode_label);
        assert_eq!(left_perf.frames_in_flight, right_perf.frames_in_flight);
        assert_eq!(left_perf.cycles, right_perf.cycles);
        assert_eq!(left_perf.platform_name, right_perf.platform_name);
        assert_eq!(left_perf.hardware_label, right_perf.hardware_label);
        assert_eq!(
            left_perf.hardware_pref_label,
            right_perf.hardware_pref_label
        );

        let left_oam = left.ui_data.oam_debug.as_ref().expect("OAM data");
        let right_oam = right.ui_data.oam_debug.as_ref().expect("OAM data");
        assert_eq!(left_oam.headers, right_oam.headers);
        assert_eq!(left_oam.rows, right_oam.rows);
        assert!(!left_oam.rows.is_empty());

        let left_palette = left.ui_data.palette_debug.as_ref().expect("palette data");
        let right_palette = right.ui_data.palette_debug.as_ref().expect("palette data");
        assert_eq!(left_palette.groups.len(), right_palette.groups.len());
        assert!(!left_palette.groups.is_empty());
        for (left_group, right_group) in left_palette.groups.iter().zip(&right_palette.groups) {
            assert_eq!(left_group.title, right_group.title);
            assert_eq!(left_group.rows.len(), right_group.rows.len());
            assert!(!left_group.rows.is_empty());
            for (left_row, right_row) in left_group.rows.iter().zip(&right_group.rows) {
                assert_eq!(left_row.label, right_row.label);
                assert_eq!(left_row.colors, right_row.colors);
                assert!(!left_row.colors.is_empty());
            }
        }

        let left_rom = left.ui_data.rom_debug.as_ref().expect("ROM data");
        let right_rom = right.ui_data.rom_debug.as_ref().expect("ROM data");
        assert_eq!(left_rom.sections.len(), right_rom.sections.len());
        assert!(!left_rom.sections.is_empty());
        for (left_section, right_section) in left_rom.sections.iter().zip(&right_rom.sections) {
            assert_eq!(left_section.heading, right_section.heading);
            assert_eq!(left_section.fields, right_section.fields);
            assert!(!left_section.fields.is_empty());
        }

        match (
            left.ui_data.input_debug.as_ref(),
            right.ui_data.input_debug.as_ref(),
        ) {
            (Some(left_input), Some(right_input)) => {
                assert_eq!(left_input.sections, right_input.sections);
                assert!(!left_input.sections.is_empty());
                assert_eq!(
                    left_input.progress_bars.len(),
                    right_input.progress_bars.len()
                );
                for ((left_name, left_value), (right_name, right_value)) in left_input
                    .progress_bars
                    .iter()
                    .zip(&right_input.progress_bars)
                {
                    assert_eq!(left_name, right_name);
                    assert_eq!(left_value.to_bits(), right_value.to_bits());
                }
            }
            (None, None) => {}
            _ => panic!("input data presence differs"),
        }

        match (
            left.ui_data.graphics_data.as_ref(),
            right.ui_data.graphics_data.as_ref(),
        ) {
            (
                Some(crate::debug::ConsoleGraphicsData::Sega8(left)),
                Some(crate::debug::ConsoleGraphicsData::Sega8(right)),
            ) => {
                assert_eq!(left.system, right.system);
                assert_bytes_equal("VRAM", &left.vram, &right.vram);
                assert_bytes_equal("CRAM", &left.cram, &right.cram);
                assert_bytes_equal("OAM", &left.oam, &right.oam);
                assert!(!left.vram.is_empty());
                assert!(!left.cram.is_empty());
                assert!(!left.oam.is_empty());
                assert_eq!(left.registers, right.registers);
                assert_eq!(left.status, right.status);
                assert_eq!(left.address, right.address);
                assert_eq!(left.code, right.code);
                assert_eq!(left.v_counter, right.v_counter);
                assert_eq!(left.h_counter, right.h_counter);
                assert_eq!(left.scanline, right.scanline);
                assert_eq!(left.scanline_cycle, right.scanline_cycle);
                assert_eq!(left.line_counter, right.line_counter);
                assert_eq!(left.frame_interrupt_enabled, right.frame_interrupt_enabled);
                assert_eq!(left.line_interrupt_enabled, right.line_interrupt_enabled);
                assert_eq!(left.interrupt_pending, right.interrupt_pending);
                assert_eq!(left.line_interrupt_pending, right.line_interrupt_pending);
                assert_eq!(left.display_enabled, right.display_enabled);
                assert_eq!(left.tms9918_mode, right.tms9918_mode);
                assert_eq!(left.sprite_table_base, right.sprite_table_base);
                assert_eq!(left.mode4, right.mode4);
                assert_eq!(left.tms9918, right.tms9918);
            }
            (
                Some(crate::debug::ConsoleGraphicsData::Gba(left)),
                Some(crate::debug::ConsoleGraphicsData::Gba(right)),
            ) => {
                assert_bytes_equal("GBA VRAM", &left.vram, &right.vram);
                assert_bytes_equal("GBA palette RAM", &left.palette_ram, &right.palette_ram);
                assert_bytes_equal("GBA OAM", &left.oam, &right.oam);
                assert!(!left.vram.is_empty());
                assert!(!left.palette_ram.is_empty());
                assert!(!left.oam.is_empty());
                assert_eq!(left.ppu, right.ppu);
            }
            _ => panic!("graphics data family differs"),
        }

        let left_disassembly = left
            .ui_data
            .disassembly_view
            .as_ref()
            .expect("disassembly data");
        let right_disassembly = right
            .ui_data
            .disassembly_view
            .as_ref()
            .expect("disassembly data");
        assert_eq!(left_disassembly.pc, right_disassembly.pc);
        assert_eq!(left_disassembly.mapping, right_disassembly.mapping);
        assert_eq!(
            left_disassembly.is_navigation_target,
            right_disassembly.is_navigation_target
        );
        assert_eq!(
            left_disassembly.is_static_target,
            right_disassembly.is_static_target
        );
        assert_eq!(
            left_disassembly.location_symbol,
            right_disassembly.location_symbol
        );
        assert_eq!(left_disassembly.lines.len(), right_disassembly.lines.len());
        assert!(!left_disassembly.lines.is_empty());
        for (left_line, right_line) in left_disassembly.lines.iter().zip(&right_disassembly.lines) {
            assert_eq!(left_line.address, right_line.address);
            assert_eq!(left_line.storage_offset, right_line.storage_offset);
            assert_eq!(left_line.bank, right_line.bank);
            assert_eq!(left_line.symbol, right_line.symbol);
            assert_eq!(left_line.control_target, right_line.control_target);
            assert_eq!(
                left_line.control_target_storage,
                right_line.control_target_storage
            );
            assert_eq!(
                left_line.control_target_bank,
                right_line.control_target_bank
            );
            assert_eq!(
                left_line.control_target_symbol,
                right_line.control_target_symbol
            );
            assert_eq!(left_line.source, right_line.source);
            assert_bytes_equal("disassembly bytes", &left_line.bytes, &right_line.bytes);
            assert_eq!(left_line.mnemonic, right_line.mnemonic);
        }
        assert_eq!(left_disassembly.breakpoints, right_disassembly.breakpoints);
        assert_eq!(
            left_disassembly.one_shot_breakpoints,
            right_disassembly.one_shot_breakpoints
        );
        assert_eq!(
            left_disassembly.rom_breakpoints,
            right_disassembly.rom_breakpoints
        );
        assert_eq!(
            left_disassembly.hit_rom_breakpoint,
            right_disassembly.hit_rom_breakpoint
        );

        let left_memory = left.ui_data.memory_page.as_ref().expect("memory page");
        let right_memory = right.ui_data.memory_page.as_ref().expect("memory page");
        assert_eq!(left_memory, right_memory);
        assert!(!left_memory.is_empty());
        let left_memory_search = left
            .ui_data
            .memory_search_results
            .as_ref()
            .expect("memory search");
        let right_memory_search = right
            .ui_data
            .memory_search_results
            .as_ref()
            .expect("memory search");
        assert_eq!(left_memory_search.len(), right_memory_search.len());
        assert!(!left_memory_search.is_empty());
        for (left_match, right_match) in left_memory_search.iter().zip(right_memory_search) {
            assert_eq!(left_match.address, right_match.address);
            assert_bytes_equal(
                "memory search match",
                &left_match.matched_bytes,
                &right_match.matched_bytes,
            );
        }

        let left_rom_page = left.ui_data.rom_page.as_ref().expect("ROM page");
        let right_rom_page = right.ui_data.rom_page.as_ref().expect("ROM page");
        assert_eq!(left_rom_page, right_rom_page);
        assert!(!left_rom_page.is_empty());
        assert_eq!(left.ui_data.rom_size, right.ui_data.rom_size);
        assert!(left.ui_data.rom_size > 0);
        let left_rom_search = left
            .ui_data
            .rom_search_results
            .as_ref()
            .expect("ROM search");
        let right_rom_search = right
            .ui_data
            .rom_search_results
            .as_ref()
            .expect("ROM search");
        assert_eq!(left_rom_search.len(), right_rom_search.len());
        assert!(!left_rom_search.is_empty());
        for (left_match, right_match) in left_rom_search.iter().zip(right_rom_search) {
            assert_eq!(left_match.offset, right_match.offset);
            assert_bytes_equal(
                "ROM search match",
                &left_match.matched_bytes,
                &right_match.matched_bytes,
            );
        }

        let left_trace = left
            .ui_data
            .instruction_trace
            .as_ref()
            .expect("instruction trace");
        let right_trace = right
            .ui_data
            .instruction_trace
            .as_ref()
            .expect("instruction trace");
        assert!(left_trace.enabled);
        assert_eq!(left_trace.enabled, right_trace.enabled);
        assert_eq!(left_trace.capacity, right_trace.capacity);
        assert_eq!(left_trace.retained, right_trace.retained);
        assert!(left_trace.retained > 0);
        assert_eq!(left_trace.oldest_sequence, right_trace.oldest_sequence);
        assert_eq!(left_trace.newest_sequence, right_trace.newest_sequence);
        assert_eq!(left_trace.entries, right_trace.entries);
        assert!(!left_trace.entries.is_empty());
    }

    fn assert_active_audio_results_match(left: &FrameResult, right: &FrameResult) {
        assert_frame_results_match(left, right);
        assert_rich_ui_data_match(left, right);
        assert!(left.ui_data.input_debug.is_some());
        assert!(!left.audio_samples.is_empty());
        assert_eq!(left.audio_samples.len() % 2, 0);
        assert!(left.audio_samples.iter().any(|sample| sample.abs() > 0.05));
        assert_eq!(left.audio_semantic_frames.len(), 1);
        let tone_0 = left.audio_semantic_frames[0]
            .voices
            .iter()
            .find(|voice| voice.channel == AudioChannelId(0))
            .expect("semantic frame should contain PSG tone 0");
        assert!(tone_0.active);
        assert!(tone_0.pitch_hz.is_some_and(|pitch| pitch > 0.0));
        assert!(tone_0.level.is_some_and(|level| level > 0.0));
        assert!(left.audio_timeline_discontinuities.is_empty());

        let left_apu = left
            .ui_data
            .apu_debug
            .as_ref()
            .expect("active APU capture should publish debug data");
        let right_apu = right
            .ui_data
            .apu_debug
            .as_ref()
            .expect("active APU capture should publish debug data");
        assert_eq!(left_apu.master_lines, right_apu.master_lines);
        assert_f32_equal(
            "APU master waveform",
            &left_apu.master_waveform,
            &right_apu.master_waveform,
        );
        assert_eq!(left_apu.master_waveform.len(), 512);
        assert!(
            left_apu
                .master_waveform
                .iter()
                .any(|sample| sample.abs() > 0.05)
        );
        assert_eq!(left_apu.channels.len(), right_apu.channels.len());
        for (left_channel, right_channel) in left_apu.channels.iter().zip(&right_apu.channels) {
            assert_eq!(left_channel.name, right_channel.name);
            assert_eq!(left_channel.enabled, right_channel.enabled);
            assert_eq!(left_channel.muted, right_channel.muted);
            assert_eq!(left_channel.register_lines, right_channel.register_lines);
            assert_eq!(left_channel.detail_line, right_channel.detail_line);
            assert_f32_equal(
                "APU channel waveform",
                &left_channel.waveform,
                &right_channel.waveform,
            );
        }
        assert!(left_apu.channels[0].enabled);
        assert_eq!(left_apu.channels[0].waveform.len(), 512);
        assert!(
            left_apu.channels[0]
                .waveform
                .iter()
                .any(|sample| sample.abs() > 0.05)
        );
    }

    fn assert_gba_results_match(left: &FrameResult, right: &FrameResult) {
        assert_frame_results_match(left, right);
        assert_rich_ui_data_match(left, right);
        assert!(left.ui_data.input_debug.is_none());
        assert!(!left.audio_samples.is_empty());
        assert_eq!(left.audio_samples.len() % 2, 0);
        assert_eq!(left.audio_semantic_frames.len(), 1);
        assert!(left.audio_timeline_discontinuities.is_empty());

        let left_apu = left.ui_data.apu_debug.as_ref().expect("GBA APU data");
        let right_apu = right.ui_data.apu_debug.as_ref().expect("GBA APU data");
        assert_eq!(left_apu.master_lines, right_apu.master_lines);
        assert_f32_equal(
            "GBA APU master waveform",
            &left_apu.master_waveform,
            &right_apu.master_waveform,
        );
        assert_eq!(left_apu.master_waveform.len(), 512);
        assert_eq!(left_apu.channels.len(), right_apu.channels.len());
        assert_eq!(left_apu.channels.len(), 6);
        for (left_channel, right_channel) in left_apu.channels.iter().zip(&right_apu.channels) {
            assert_eq!(left_channel.name, right_channel.name);
            assert_eq!(left_channel.enabled, right_channel.enabled);
            assert_eq!(left_channel.muted, right_channel.muted);
            assert_eq!(left_channel.register_lines, right_channel.register_lines);
            assert_eq!(left_channel.detail_line, right_channel.detail_line);
            assert_f32_equal(
                "GBA APU channel waveform",
                &left_channel.waveform,
                &right_channel.waveform,
            );
            assert_eq!(left_channel.waveform.len(), 512);
        }
    }

    #[wasm_bindgen_test]
    fn wasm_sms_detached_stepframes_matches_control_and_selects_projection() {
        let control = sms_thread();
        let subject = sms_thread();
        subject
            .inner
            .borrow_mut()
            .speculation
            .force_frames_for_test(1);

        control.send(EmuCommand::StepFrames(Box::new(frame_input())));
        subject.send(EmuCommand::StepFrames(Box::new(frame_input())));

        {
            let inner = control.inner.borrow();
            assert_eq!(inner.speculation.committed_frames_for_test(), 1);
        }
        {
            let inner = subject.inner.borrow();
            assert_eq!(inner.speculation.completed_runs_for_test(), 1);
            assert_eq!(inner.speculation.committed_frames_for_test(), 1);
        }
        let control_result = control.try_recv_frame().unwrap();
        let subject_result = subject.try_recv_frame().unwrap();
        assert_active_audio_results_match(&control_result, &subject_result);

        let expected_projection = {
            let inner = control.inner.borrow();
            let mut detached = inner.backend.fork_detached_for_speculation().unwrap();
            detached.disable_audio_output();
            assert!(detached.step_frames(1));
            detached.framebuffer().to_vec()
        };
        let control_primary = observe_primary(&control);
        let subject_primary = observe_primary(&subject);
        assert_primary_matches(&control_primary, &subject_primary);
        assert_eq!(
            subject.shared_framebuffer.load_full().unwrap().as_slice(),
            expected_projection.as_slice()
        );
        assert_eq!(
            control.shared_framebuffer.load_full().unwrap().as_slice(),
            control_primary.framebuffer.as_slice()
        );
    }

    fn assert_detached_fallback(wrong_framebuffer_len: bool) {
        let control = sms_thread();
        let subject = sms_thread();
        {
            let mut inner = subject.inner.borrow_mut();
            inner.speculation.force_frames_for_test(1);
            if wrong_framebuffer_len {
                inner.speculation.force_wrong_framebuffer_len_for_test();
            } else {
                inner.speculation.force_operational_failure_for_test();
            }
        }

        control.send(EmuCommand::StepFrames(Box::new(frame_input())));
        subject.send(EmuCommand::StepFrames(Box::new(frame_input())));

        {
            let inner = control.inner.borrow();
            assert_eq!(inner.speculation.committed_frames_for_test(), 1);
        }
        {
            let inner = subject.inner.borrow();
            assert_eq!(inner.speculation.completed_runs_for_test(), 0);
            assert_eq!(inner.speculation.committed_frames_for_test(), 1);
        }
        let control_result = control.try_recv_frame().unwrap();
        let subject_result = subject.try_recv_frame().unwrap();
        assert_active_audio_results_match(&control_result, &subject_result);
        let control_primary = observe_primary(&control);
        let subject_primary = observe_primary(&subject);
        assert_primary_matches(&control_primary, &subject_primary);
        assert_eq!(
            subject.shared_framebuffer.load_full().unwrap().as_slice(),
            subject_primary.framebuffer.as_slice()
        );
    }

    #[wasm_bindgen_test]
    fn wasm_sms_detached_stepframes_falls_back_on_operational_or_size_failure() {
        assert_detached_fallback(false);
        assert_detached_fallback(true);
    }

    #[wasm_bindgen_test]
    fn wasm_gba_detached_stepframes_matches_control_and_selects_projection() {
        let control = gba_thread(false);
        let subject = gba_thread(false);
        subject
            .inner
            .borrow_mut()
            .speculation
            .force_frames_for_test(1);

        control.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
        subject.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
        assert_eq!(
            control
                .inner
                .borrow()
                .speculation
                .committed_frames_for_test(),
            1
        );
        assert_eq!(
            subject.inner.borrow().speculation.completed_runs_for_test(),
            1
        );
        assert_eq!(
            subject
                .inner
                .borrow()
                .speculation
                .committed_frames_for_test(),
            1
        );
        let control_result = control.try_recv_frame().unwrap();
        let subject_result = subject.try_recv_frame().unwrap();
        assert_gba_results_match(&control_result, &subject_result);

        let expected_projection = {
            let inner = control.inner.borrow();
            let mut detached = inner.backend.fork_detached_for_speculation().unwrap();
            detached.disable_audio_output();
            assert!(detached.step_frames(1));
            detached.framebuffer().to_vec()
        };
        let control_primary = observe_primary(&control);
        let subject_primary = observe_primary(&subject);
        assert_primary_matches(&control_primary, &subject_primary);
        assert!(control_primary.potentially_dirty);
        assert_eq!(control_primary.gba_rtc, Some(gba_rtc()));
        assert_eq!(subject_primary.gba_rtc, Some(gba_rtc()));
        let battery = control_primary.battery_bytes.as_ref().unwrap();
        assert_eq!(battery, &gba_sram_bytes(battery.len()));
        assert_eq!(
            subject.shared_framebuffer.load_full().unwrap().as_slice(),
            expected_projection.as_slice()
        );
        assert_eq!(
            control.shared_framebuffer.load_full().unwrap().as_slice(),
            control_primary.framebuffer.as_slice()
        );
    }

    fn assert_gba_detached_fallback(wrong_framebuffer_len: bool) {
        let control = gba_thread(false);
        let subject = gba_thread(false);
        {
            let mut inner = subject.inner.borrow_mut();
            inner.speculation.force_frames_for_test(1);
            if wrong_framebuffer_len {
                inner.speculation.force_wrong_framebuffer_len_for_test();
            } else {
                inner.speculation.force_operational_failure_for_test();
            }
        }
        control.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
        subject.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
        assert_eq!(
            control
                .inner
                .borrow()
                .speculation
                .committed_frames_for_test(),
            1
        );
        assert_eq!(
            subject.inner.borrow().speculation.completed_runs_for_test(),
            0
        );
        assert_eq!(
            subject
                .inner
                .borrow()
                .speculation
                .committed_frames_for_test(),
            1
        );
        let control_result = control.try_recv_frame().unwrap();
        let subject_result = subject.try_recv_frame().unwrap();
        assert_gba_results_match(&control_result, &subject_result);
        let control_primary = observe_primary(&control);
        let subject_primary = observe_primary(&subject);
        assert_primary_matches(&control_primary, &subject_primary);
        assert!(control_primary.potentially_dirty);
        assert_eq!(control_primary.gba_rtc, Some(gba_rtc()));
        assert_eq!(subject_primary.gba_rtc, Some(gba_rtc()));
        let battery = control_primary.battery_bytes.as_ref().unwrap();
        assert_eq!(battery, &gba_sram_bytes(battery.len()));
        assert_eq!(
            subject.shared_framebuffer.load_full().unwrap().as_slice(),
            subject_primary.framebuffer.as_slice()
        );
    }

    #[wasm_bindgen_test]
    fn wasm_gba_detached_stepframes_falls_back_on_operational_or_size_failure() {
        assert_gba_detached_fallback(false);
        assert_gba_detached_fallback(true);
    }

    fn capture_terminal_writes(
        thread: &EmuThread,
    ) -> (CapturedBatteryWrites, Vec<crate::platform::SaveWrite>) {
        let mut inner = thread.inner.borrow_mut();
        let Inner {
            backend,
            recovery,
            speculation,
            pending_storage,
            ..
        } = &mut *inner;
        assert!(pending_storage.is_none());
        let ready = speculation.prepare_terminal_persistence();
        EmuThread::capture_terminal_save_writes(ready, backend, recovery, true).unwrap()
    }

    #[wasm_bindgen_test]
    fn wasm_sms_detached_terminal_capture_matches_control_without_committing_browser_storage() {
        let control = sms_thread_with_recovery(true);
        let subject = sms_thread_with_recovery(true);
        subject
            .inner
            .borrow_mut()
            .speculation
            .force_frames_for_test(1);

        control.send(EmuCommand::StepFrames(Box::new(frame_input())));
        subject.send(EmuCommand::StepFrames(Box::new(frame_input())));
        let control_result = control.try_recv_frame().unwrap();
        let subject_result = subject.try_recv_frame().unwrap();
        assert_active_audio_results_match(&control_result, &subject_result);
        let control_primary = observe_primary(&control);
        let subject_primary = observe_primary(&subject);
        assert_primary_matches(&control_primary, &subject_primary);

        let (control_capture, control_writes) = capture_terminal_writes(&control);
        let (subject_capture, subject_writes) = capture_terminal_writes(&subject);
        let write_parts = |writes: &[crate::platform::SaveWrite]| {
            writes
                .iter()
                .map(|write| {
                    let (key, data) = write.parts_for_test();
                    (key.to_owned(), data.to_vec())
                })
                .collect::<Vec<_>>()
        };
        let control_parts = write_parts(&control_writes);
        let subject_parts = write_parts(&subject_writes);
        assert_eq!(control_parts, subject_parts);
        assert_eq!(control_parts.len(), 2);
        assert!(control_parts.iter().all(|(key, _)| !key.ends_with(".sav")));
        assert!(control_capture.path.is_none());
        assert!(subject_capture.path.is_none());
        assert_eq!(control_capture.generation, subject_capture.generation);
        assert!(control_capture.recovery_path.is_some());
        assert_eq!(control_capture.recovery_path, subject_capture.recovery_path);

        let (system, discriminator, media_sha256, component_sha256) = {
            let inner = control.inner.borrow();
            (
                inner.backend.system().storage_subdir().to_owned(),
                inner.backend.recovery_discriminator(),
                inner.backend.rom_hash(),
                inner.backend.battery_component_hash(),
            )
        };
        let generation = crate::save_paths::recovery_state::decode_battery_generation(
            &control_parts[0].1,
            media_sha256,
        )
        .expect("terminal batch should contain a battery generation witness");
        assert_eq!(generation, control_capture.generation);
        assert_eq!(generation.component_sha256, component_sha256);
        assert_eq!(
            generation.component_sha256,
            crate::save_paths::recovery_state::canonical_battery_component_hash(&[])
        );
        let envelope = crate::save_paths::recovery_state::decode_recovery_state(
            &control_parts[1].1,
            crate::save_paths::recovery_state::RecoveryStateIdentity {
                system: &system,
                discriminator: &discriminator,
                media_sha256,
            },
        )
        .expect("terminal batch should contain a recovery-state envelope");
        assert_eq!(envelope.system, system);
        assert_eq!(envelope.discriminator, discriminator);
        assert_eq!(envelope.media_sha256, media_sha256);
        assert_eq!(
            envelope.battery,
            crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
                generation: generation.generation,
                component_sha256,
            }
        );
        assert_eq!(envelope.native_payload, control_primary.state);
        assert!(control.inner.borrow().pending_storage.is_none());
        assert!(subject.inner.borrow().pending_storage.is_none());
        assert_eq!(
            control.inner.borrow().speculation.completed_runs_for_test(),
            0
        );
        assert_eq!(
            subject.inner.borrow().speculation.completed_runs_for_test(),
            1
        );
    }

    #[wasm_bindgen_test]
    fn wasm_gba_detached_terminal_capture_matches_control_without_committing_browser_storage() {
        let control = gba_thread(true);
        let subject = gba_thread(true);
        subject
            .inner
            .borrow_mut()
            .speculation
            .force_frames_for_test(1);

        control.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
        subject.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
        let control_result = control.try_recv_frame().unwrap();
        let subject_result = subject.try_recv_frame().unwrap();
        assert_gba_results_match(&control_result, &subject_result);
        let control_primary = observe_primary(&control);
        let subject_primary = observe_primary(&subject);
        assert_primary_matches(&control_primary, &subject_primary);
        assert!(control_primary.potentially_dirty);
        assert_eq!(control_primary.gba_rtc, Some(gba_rtc()));

        let (control_capture, control_writes) = capture_terminal_writes(&control);
        let (subject_capture, subject_writes) = capture_terminal_writes(&subject);
        let write_parts = |writes: &[crate::platform::SaveWrite]| {
            writes
                .iter()
                .map(|write| {
                    let (key, data) = write.parts_for_test();
                    (key.to_owned(), data.to_vec())
                })
                .collect::<Vec<_>>()
        };
        let control_parts = write_parts(&control_writes);
        let subject_parts = write_parts(&subject_writes);
        assert_eq!(control_parts, subject_parts);
        assert_eq!(control_parts.len(), 3);
        assert!(control_capture.path.is_some());
        assert_eq!(control_capture.path, subject_capture.path);
        assert_eq!(control_capture.generation, subject_capture.generation);
        assert!(control_capture.recovery_path.is_some());
        assert_eq!(control_capture.recovery_path, subject_capture.recovery_path);
        assert_eq!(
            control_parts[0].1,
            control_primary.battery_bytes.clone().unwrap()
        );

        let (system, discriminator, media_sha256, component_sha256) = {
            let inner = control.inner.borrow();
            (
                inner.backend.system().storage_subdir().to_owned(),
                inner.backend.recovery_discriminator(),
                inner.backend.rom_hash(),
                inner.backend.battery_component_hash(),
            )
        };
        let generation = crate::save_paths::recovery_state::decode_battery_generation(
            &control_parts[1].1,
            media_sha256,
        )
        .expect("terminal batch should contain a battery generation witness");
        assert_eq!(generation, control_capture.generation);
        assert_eq!(generation.component_sha256, component_sha256);
        let envelope = crate::save_paths::recovery_state::decode_recovery_state(
            &control_parts[2].1,
            crate::save_paths::recovery_state::RecoveryStateIdentity {
                system: &system,
                discriminator: &discriminator,
                media_sha256,
            },
        )
        .expect("terminal batch should contain a recovery-state envelope");
        assert_eq!(envelope.system, system);
        assert_eq!(envelope.discriminator, discriminator);
        assert_eq!(envelope.media_sha256, media_sha256);
        assert_eq!(
            envelope.battery,
            crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
                generation: generation.generation,
                component_sha256,
            }
        );
        assert_eq!(envelope.native_payload, control_primary.state);
        assert!(control.inner.borrow().pending_storage.is_none());
        assert!(subject.inner.borrow().pending_storage.is_none());
        assert_eq!(
            control.inner.borrow().speculation.completed_runs_for_test(),
            0
        );
        assert_eq!(
            subject.inner.borrow().speculation.completed_runs_for_test(),
            1
        );
    }

    #[cfg(feature = "wasm-browser-tests")]
    struct BrowserObservation {
        frame_result: FrameResult,
        primary: PrimaryObservation,
        terminal_responses: Vec<String>,
        stored_entries: Vec<(String, Vec<u8>)>,
    }

    #[cfg(feature = "wasm-browser-tests")]
    async fn wait_for_browser_terminal(thread: &EmuThread) -> Vec<String> {
        let deadline = js_sys::Date::now() + 10_000.0;
        let mut responses = Vec::new();
        loop {
            while let Some(response) = thread.try_recv_response() {
                match response {
                    EmuResponse::SramFlushed(path) => {
                        responses.push(format!("sram:{}", path.unwrap_or_default()));
                    }
                    EmuResponse::RecoverySaved(path) => {
                        responses.push(format!("recovery:{}", path.display()));
                    }
                    EmuResponse::ShutdownComplete => {
                        responses.push("shutdown".to_string());
                        assert_eq!(responses.len(), 3);
                        assert!(responses[0].starts_with("sram:"));
                        assert!(responses[1].starts_with("recovery:"));
                        assert_eq!(responses[2], "shutdown");
                        assert!(thread.try_recv_response().is_none());
                        return responses;
                    }
                    _ => panic!("unexpected response during browser shutdown"),
                }
            }
            assert!(
                js_sys::Date::now() < deadline,
                "browser persistence timed out"
            );
            browser_test_delay(5)
                .await
                .expect("browser timer should resolve");
        }
    }

    #[cfg(feature = "wasm-browser-tests")]
    fn assert_browser_gba_entries(
        thread: &EmuThread,
        primary: &PrimaryObservation,
        entries: &[(String, Vec<u8>)],
    ) {
        assert_eq!(entries.len(), 3);
        let battery = primary
            .battery_bytes
            .as_ref()
            .expect("GBA battery bytes should be present");
        assert_eq!(
            entries
                .iter()
                .filter(|(_, data)| data.as_slice() == battery.as_slice())
                .count(),
            1
        );

        let inner = thread.inner.borrow();
        let media_sha256 = inner.backend.rom_hash();
        let component_sha256 = inner.backend.battery_component_hash();
        let system = inner.backend.system().storage_subdir().to_owned();
        let discriminator = inner.backend.recovery_discriminator();
        let generations = entries
            .iter()
            .filter_map(|(_, data)| {
                crate::save_paths::recovery_state::decode_battery_generation(data, media_sha256)
            })
            .collect::<Vec<_>>();
        assert_eq!(generations.len(), 1);
        assert_eq!(generations[0].component_sha256, component_sha256);
        let envelopes = entries
            .iter()
            .filter_map(|(_, data)| {
                crate::save_paths::recovery_state::decode_recovery_state(
                    data,
                    crate::save_paths::recovery_state::RecoveryStateIdentity {
                        system: &system,
                        discriminator: &discriminator,
                        media_sha256,
                    },
                )
                .ok()
            })
            .collect::<Vec<_>>();
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].system, system);
        assert_eq!(envelopes[0].discriminator, discriminator);
        assert_eq!(envelopes[0].media_sha256, media_sha256);
        assert_eq!(
            envelopes[0].battery,
            crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
                generation: generations[0].generation,
                component_sha256,
            }
        );
        assert_eq!(envelopes[0].native_payload, primary.state);
    }

    #[cfg(feature = "wasm-browser-tests")]
    async fn run_browser_gba_transaction(force_detached: bool) -> BrowserObservation {
        crate::platform::clear_browser_storage_for_test()
            .await
            .expect("browser storage should clear");
        let thread = gba_thread(true);
        if force_detached {
            thread
                .inner
                .borrow_mut()
                .speculation
                .force_frames_for_test(1);
        }

        thread.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
        let frame_result = thread
            .try_recv_frame()
            .expect("StepFrames should publish one result");
        let expected_projection = force_detached.then(|| {
            let inner = thread.inner.borrow();
            let mut detached = inner.backend.fork_detached_for_speculation().unwrap();
            detached.disable_audio_output();
            assert!(detached.step_frames(1));
            detached.framebuffer().to_vec()
        });
        let primary = observe_primary(&thread);
        assert!(primary.potentially_dirty);
        assert_eq!(primary.gba_rtc, Some(gba_rtc()));
        let battery = primary.battery_bytes.as_ref().unwrap();
        assert_eq!(battery, &gba_sram_bytes(battery.len()));
        assert_eq!(
            thread
                .inner
                .borrow()
                .speculation
                .committed_frames_for_test(),
            1
        );
        assert_eq!(
            thread.inner.borrow().speculation.completed_runs_for_test(),
            if force_detached { 1 } else { 0 }
        );
        let published = thread.shared_framebuffer.load_full().unwrap();
        if let Some(expected_projection) = expected_projection {
            assert_eq!(published.as_slice(), expected_projection.as_slice());
        } else {
            assert_eq!(published.as_slice(), primary.framebuffer.as_slice());
        }

        thread.send(EmuCommand::Shutdown);
        assert!(thread.inner.borrow().pending_storage.is_some());
        assert!(thread.try_recv_response().is_none());
        let terminal_responses = wait_for_browser_terminal(&thread).await;
        assert_eq!(
            thread.inner.borrow().speculation.completed_runs_for_test(),
            if force_detached { 1 } else { 0 }
        );
        assert!(thread.inner.borrow().pending_storage.is_none());
        let stored_entries = crate::platform::fresh_browser_storage_entries_for_test()
            .await
            .expect("production IndexedDB reload should match stored entries");
        assert_browser_gba_entries(&thread, &primary, &stored_entries);

        BrowserObservation {
            frame_result,
            primary,
            terminal_responses,
            stored_entries,
        }
    }

    #[cfg(feature = "wasm-browser-tests")]
    #[wasm_bindgen_test]
    async fn wasm_gba_browser_indexeddb_transaction_matches_detached_control() {
        let control = run_browser_gba_transaction(false).await;
        let subject = run_browser_gba_transaction(true).await;

        assert_gba_results_match(&control.frame_result, &subject.frame_result);
        assert_primary_matches(&control.primary, &subject.primary);
        assert_eq!(control.terminal_responses, subject.terminal_responses);
        assert_eq!(control.stored_entries, subject.stored_entries);
        crate::platform::clear_browser_storage_for_test()
            .await
            .expect("browser storage cleanup should complete");
    }

    #[cfg(feature = "wasm-browser-tests")]
    fn assert_browser_sms_entries(
        thread: &EmuThread,
        primary: &PrimaryObservation,
        entries: &[(String, Vec<u8>)],
    ) {
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|(key, _)| !key.ends_with(".sav")));
        assert!(
            entries
                .iter()
                .all(|(key, _)| !key.starts_with("zeff-sram-v2:"))
        );
        assert!(primary.battery_bytes.is_none());

        let inner = thread.inner.borrow();
        assert!(!inner.backend.save_ram_kind().is_battery_backed());
        let media_sha256 = inner.backend.rom_hash();
        let component_sha256 = inner.backend.battery_component_hash();
        assert_eq!(
            component_sha256,
            crate::save_paths::recovery_state::canonical_battery_component_hash(&[])
        );
        let system = inner.backend.system().storage_subdir().to_owned();
        let discriminator = inner.backend.recovery_discriminator();
        let generation_path =
            crate::save_paths::recovery_state::battery_generation_path(&system, media_sha256)
                .unwrap();
        let recovery_path = crate::save_paths::recovery_state::recovery_state_path(
            &system,
            inner.backend.system().state_extension(),
            media_sha256,
        )
        .unwrap();
        let mut expected_keys = vec![
            format!("zeff-state-{}", generation_path.display()),
            format!("zeff-state-{}", recovery_path.display()),
        ];
        expected_keys.sort();
        assert_eq!(
            entries
                .iter()
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>(),
            expected_keys
        );
        let generations = entries
            .iter()
            .filter_map(|(_, data)| {
                crate::save_paths::recovery_state::decode_battery_generation(data, media_sha256)
            })
            .collect::<Vec<_>>();
        assert_eq!(generations.len(), 1);
        assert_eq!(generations[0].generation, 0);
        assert_eq!(generations[0].component_sha256, component_sha256);
        let envelopes = entries
            .iter()
            .filter_map(|(_, data)| {
                crate::save_paths::recovery_state::decode_recovery_state(
                    data,
                    crate::save_paths::recovery_state::RecoveryStateIdentity {
                        system: &system,
                        discriminator: &discriminator,
                        media_sha256,
                    },
                )
                .ok()
            })
            .collect::<Vec<_>>();
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].system, system);
        assert_eq!(envelopes[0].discriminator, discriminator);
        assert_eq!(envelopes[0].media_sha256, media_sha256);
        assert_eq!(
            envelopes[0].battery,
            crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
                generation: 0,
                component_sha256,
            }
        );
        assert_eq!(envelopes[0].native_payload, primary.state);
    }

    #[cfg(feature = "wasm-browser-tests")]
    async fn run_browser_sms_transaction(force_detached: bool) -> BrowserObservation {
        crate::platform::clear_browser_storage_for_test()
            .await
            .expect("browser storage should clear");
        let thread = sms_thread_with_recovery(true);
        if force_detached {
            thread
                .inner
                .borrow_mut()
                .speculation
                .force_frames_for_test(1);
        }

        thread.send(EmuCommand::StepFrames(Box::new(frame_input())));
        let frame_result = thread
            .try_recv_frame()
            .expect("StepFrames should publish one result");
        let expected_projection = force_detached.then(|| {
            let inner = thread.inner.borrow();
            let mut detached = inner.backend.fork_detached_for_speculation().unwrap();
            detached.disable_audio_output();
            assert!(detached.step_frames(1));
            detached.framebuffer().to_vec()
        });
        let primary = observe_primary(&thread);
        assert!(primary.potentially_dirty);
        assert!(primary.battery_bytes.is_none());
        assert_eq!(
            thread
                .inner
                .borrow()
                .speculation
                .committed_frames_for_test(),
            1
        );
        assert_eq!(
            thread.inner.borrow().speculation.completed_runs_for_test(),
            if force_detached { 1 } else { 0 }
        );
        let published = thread.shared_framebuffer.load_full().unwrap();
        if let Some(expected_projection) = expected_projection {
            assert_eq!(published.as_slice(), expected_projection.as_slice());
        } else {
            assert_eq!(published.as_slice(), primary.framebuffer.as_slice());
        }

        let expected_recovery_path = {
            let inner = thread.inner.borrow();
            crate::save_paths::recovery_state::recovery_state_path(
                inner.backend.system().storage_subdir(),
                inner.backend.system().state_extension(),
                inner.backend.rom_hash(),
            )
            .unwrap()
        };
        thread.send(EmuCommand::Shutdown);
        assert!(thread.inner.borrow().pending_storage.is_some());
        assert!(thread.try_recv_response().is_none());
        let terminal_responses = wait_for_browser_terminal(&thread).await;
        assert_eq!(terminal_responses[0], "sram:");
        assert_eq!(
            terminal_responses[1],
            format!("recovery:{}", expected_recovery_path.display())
        );
        assert_eq!(
            thread.inner.borrow().speculation.completed_runs_for_test(),
            if force_detached { 1 } else { 0 }
        );
        assert!(thread.inner.borrow().pending_storage.is_none());
        let stored_entries = crate::platform::fresh_browser_storage_entries_for_test()
            .await
            .expect("production IndexedDB reload should match stored entries");
        assert_browser_sms_entries(&thread, &primary, &stored_entries);

        BrowserObservation {
            frame_result,
            primary,
            terminal_responses,
            stored_entries,
        }
    }

    #[cfg(feature = "wasm-browser-tests")]
    #[wasm_bindgen_test]
    async fn wasm_sms_browser_indexeddb_transaction_matches_detached_control() {
        let control = run_browser_sms_transaction(false).await;
        let subject = run_browser_sms_transaction(true).await;

        assert_active_audio_results_match(&control.frame_result, &subject.frame_result);
        assert_primary_matches(&control.primary, &subject.primary);
        assert_eq!(control.terminal_responses, subject.terminal_responses);
        assert_eq!(control.stored_entries, subject.stored_entries);
        crate::platform::clear_browser_storage_for_test()
            .await
            .expect("browser storage cleanup should complete");
    }
}
