use std::cell::RefCell;
use std::collections::VecDeque;

use zeff_emu_common::rewind::RewindBuffer;

use super::recovery::{RecoveryCandidate, RecoveryCoordinator, should_load_recovery};
use super::speculation::SpeculationBoundary;
use super::types::{self, EmuCommand, EmuResponse, FrameInput, FrameResult, SharedFramebuffer};
use super::{DEFAULT_REWIND_SECONDS, REWIND_CAPTURE_INTERVAL_FRAMES, WorkerRuntimeFault};
use crate::cheats::CheatPatch;
use crate::emu_backend::{CoreCapabilities, EmuBackend};
use zeff_emu_common::time::MachineTiming;

mod storage;

#[cfg(all(test, target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(all(test, target_arch = "wasm32"))]
use storage::CapturedBatteryWrites;
use storage::{BATTERY_FLUSH_INTERVAL, MAX_DEFERRED_STORAGE_COMMANDS, PendingStorage};

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
        let cmd = match (super::commands::CommonCommandContext {
            backend,
            shared_framebuffer: &self.shared_framebuffer,
            uncapped_mode,
            rewind_buffer,
            last_cheats,
            audio_recording_capture,
            pending_audio_discontinuities,
            runtime_fault,
        })
        .dispatch(cmd)
        {
            super::commands::CommonCommandDispatch::Handled(effects) => {
                if effects.potentially_dirty {
                    *battery_potentially_dirty = true;
                }
                if let Some(response) = effects.response {
                    pending_responses.push_back(response);
                }
                return;
            }
            super::commands::CommonCommandDispatch::PlatformSpecific(command) => command,
        };
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
                let potentially_dirty = super::commands::finalize_step_result(
                    &mut result,
                    debugger_mutation,
                    *audio_recording_capture,
                    pending_audio_discontinuities,
                    runtime_fault,
                    uncapped_mode,
                );
                pending_frames.push_back(result);
                if potentially_dirty {
                    *battery_potentially_dirty = true;
                }
            }
            EmuCommand::SetUncappedBatchSize(_) => {}
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
                let loaded = (super::commands::LoadFinalizationContext {
                    rewind_buffer,
                    rewind_seconds: *rewind_seconds,
                    backend,
                    cheats: last_cheats,
                    audio_recording_capture: *audio_recording_capture,
                    pending_audio_discontinuities,
                })
                .finalize(&resp, |loaded_frame_duration| {
                    *frame_duration_ns = loaded_frame_duration;
                });
                if loaded {
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
                let loaded = (super::commands::LoadFinalizationContext {
                    rewind_buffer,
                    rewind_seconds: *rewind_seconds,
                    backend,
                    cheats: last_cheats,
                    audio_recording_capture: *audio_recording_capture,
                    pending_audio_discontinuities,
                })
                .finalize(&resp, |loaded_frame_duration| {
                    *frame_duration_ns = loaded_frame_duration;
                });
                if loaded {
                    *battery_potentially_dirty = true;
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::LoadExternalStateBytes {
                state_bytes,
                buttons_pressed,
                dpad_pressed,
            } => {
                let result = backend.load_external_state_from_bytes(state_bytes);
                let resp = Self::respond_load_state(
                    backend,
                    result,
                    "bytes".to_string(),
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                let loaded = (super::commands::LoadFinalizationContext {
                    rewind_buffer,
                    rewind_seconds: *rewind_seconds,
                    backend,
                    cheats: last_cheats,
                    audio_recording_capture: *audio_recording_capture,
                    pending_audio_discontinuities,
                })
                .finalize(&resp, |loaded_frame_duration| {
                    *frame_duration_ns = loaded_frame_duration;
                });
                if loaded {
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
                        let result = backend.load_external_state_from_bytes(native_payload);
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
                let loaded = (super::commands::LoadFinalizationContext {
                    rewind_buffer,
                    rewind_seconds: *rewind_seconds,
                    backend,
                    cheats: last_cheats,
                    audio_recording_capture: *audio_recording_capture,
                    pending_audio_discontinuities,
                })
                .finalize(&resp, |loaded_frame_duration| {
                    *frame_duration_ns = loaded_frame_duration;
                });
                if loaded {
                    *battery_potentially_dirty = true;
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::CaptureReplayStart { capture_id } => {
                let resp = super::commands::capture_replay_start_response(
                    backend,
                    capture_id,
                    None,
                    || backend.replay_metadata(),
                );
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
                let loaded = (super::commands::LoadFinalizationContext {
                    rewind_buffer,
                    rewind_seconds: *rewind_seconds,
                    backend,
                    cheats: last_cheats,
                    audio_recording_capture: *audio_recording_capture,
                    pending_audio_discontinuities,
                })
                .finalize(&resp, |loaded_frame_duration| {
                    *frame_duration_ns = loaded_frame_duration;
                });
                if loaded {
                    *battery_potentially_dirty = true;
                }
                pending_responses.push_back(resp);
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
            EmuCommand::SetAudioRecordingCapture { .. }
            | EmuCommand::SetSampleRate(_)
            | EmuCommand::SetUncapped(_)
            | EmuCommand::ApplyMediaEvent(_)
            | EmuCommand::CaptureStateBytes
            | EmuCommand::CaptureExternalStateBytes
            | EmuCommand::ExecuteGuestCall(_)
            | EmuCommand::UndoGuestCall(_)
            | EmuCommand::CaptureReplayCheckpoint { .. }
            | EmuCommand::SetGameBoySerialDevice(_)
            | EmuCommand::QueueBardigunBarcodeScan(_)
            | EmuCommand::TriggerBarcodeBoyScan(_)
            | EmuCommand::RestoreGameBoyLinkState(_)
            | EmuCommand::UpdateCheats(_)
            | EmuCommand::Rewind(_)
            | EmuCommand::Reset => unreachable!("shared command escaped common dispatch"),
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
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests;
