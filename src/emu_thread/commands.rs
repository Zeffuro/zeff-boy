use crate::cheats::CheatPatch;
use crate::emu_backend::EmuBackend;
use zeff_emu_common::time::Reset as _;

use super::{
    AudioRecordingCapture, EmuCommand, EmuResponse, EmuThread, SharedFramebuffer,
    WorkerRuntimeFault,
};

pub(super) struct CommonCommandContext<'a> {
    pub(super) backend: &'a mut EmuBackend,
    pub(super) shared_framebuffer: &'a SharedFramebuffer,
    pub(super) uncapped_mode: &'a mut bool,
    pub(super) rewind_buffer: &'a mut zeff_emu_common::rewind::RewindBuffer,
    pub(super) last_cheats: &'a mut Vec<CheatPatch>,
    pub(super) audio_recording_capture: &'a mut AudioRecordingCapture,
    pub(super) pending_audio_discontinuities:
        &'a mut Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    pub(super) runtime_fault: &'a mut WorkerRuntimeFault,
}

pub(super) enum CommonCommandDispatch {
    Handled(CommonCommandEffects),
    PlatformSpecific(EmuCommand),
}

#[derive(Default)]
pub(super) struct CommonCommandEffects {
    pub(super) response: Option<EmuResponse>,
    pub(super) potentially_dirty: bool,
}

pub(super) fn finalize_step_result(
    result: &mut super::FrameResult,
    debugger_mutation: bool,
    audio_recording_capture: AudioRecordingCapture,
    pending_audio_discontinuities: &mut Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    runtime_fault: &WorkerRuntimeFault,
    uncapped_mode: &mut bool,
) -> bool {
    if debugger_mutation && runtime_fault.can_step() {
        mark_audio_discontinuity(
            audio_recording_capture,
            pending_audio_discontinuities,
            crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation,
        );
    }
    if !runtime_fault.can_step() {
        *uncapped_mode = false;
    }
    attach_audio_discontinuities(result, pending_audio_discontinuities);

    result.advanced_frames != 0 || debugger_mutation
}

pub(super) struct LoadFinalizationContext<'a> {
    pub(super) rewind_buffer: &'a mut zeff_emu_common::rewind::RewindBuffer,
    pub(super) rewind_seconds: usize,
    pub(super) backend: &'a mut EmuBackend,
    pub(super) cheats: &'a [CheatPatch],
    pub(super) audio_recording_capture: AudioRecordingCapture,
    pub(super) pending_audio_discontinuities:
        &'a mut Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
}

impl LoadFinalizationContext<'_> {
    pub(super) fn finalize(
        self,
        response: &EmuResponse,
        publish_frame_duration: impl FnOnce(u64),
    ) -> bool {
        let loaded = EmuThread::finalize_load_state(
            response,
            self.rewind_buffer,
            self.rewind_seconds,
            self.backend,
            self.cheats,
            publish_frame_duration,
        );
        if loaded {
            mark_audio_discontinuity(
                self.audio_recording_capture,
                self.pending_audio_discontinuities,
                crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad,
            );
        }
        loaded
    }
}

pub(super) fn mark_audio_discontinuity(
    audio_recording_capture: AudioRecordingCapture,
    pending_audio_discontinuities: &mut Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    reason: crate::audio_recorder::AudioTimelineDiscontinuity,
) {
    if audio_recording_capture.semantic {
        pending_audio_discontinuities.push(reason);
    }
}

pub(super) fn attach_audio_discontinuities(
    result: &mut super::FrameResult,
    pending_audio_discontinuities: &mut Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
) {
    if !result.audio_semantic_frames.is_empty() {
        result
            .audio_timeline_discontinuities
            .append(pending_audio_discontinuities);
    }
}

impl CommonCommandContext<'_> {
    pub(super) fn dispatch(mut self, command: EmuCommand) -> CommonCommandDispatch {
        let mut effects = CommonCommandEffects::default();
        match command {
            EmuCommand::SetAudioRecordingCapture {
                capture,
                acknowledged,
            } => {
                if capture.semantic && !self.audio_recording_capture.semantic {
                    self.pending_audio_discontinuities.clear();
                }
                *self.audio_recording_capture = capture;
                if let Some(acknowledged) = acknowledged {
                    let _ = acknowledged.send(());
                }
            }
            EmuCommand::SetSampleRate(rate) => self.backend.set_sample_rate(rate),
            EmuCommand::SetUncapped(on) => {
                *self.uncapped_mode = on && self.runtime_fault.can_step();
                self.backend
                    .set_apu_sample_generation_enabled(!*self.uncapped_mode);
            }
            EmuCommand::ApplyMediaEvent(event) => {
                let response = match self.backend.apply_media_event(&event) {
                    Ok(()) => match self.backend.media_slot_snapshot() {
                        Some(snapshot) => EmuResponse::MediaEventApplied {
                            event,
                            snapshot,
                            frame_count: self.backend.frame_count(),
                        },
                        None => EmuResponse::MediaEventFailed {
                            event,
                            error: "media slot disappeared after applying event".to_string(),
                        },
                    },
                    Err(error) => EmuResponse::MediaEventFailed {
                        event,
                        error: error.to_string(),
                    },
                };
                effects.potentially_dirty =
                    matches!(&response, EmuResponse::MediaEventApplied { .. });
                effects.response = Some(response);
            }
            EmuCommand::CaptureStateBytes => {
                effects.response = Some(match EmuThread::encode_current_state(self.backend) {
                    Ok(bytes) => EmuResponse::StateCaptured(bytes),
                    Err(error) => EmuResponse::StateCaptureFailed(error.to_string()),
                });
            }
            EmuCommand::ExecuteGuestCall(request) => {
                let name = request.name.clone();
                let response = match self.backend.execute_guest_call(&request) {
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
                super::types::publish_framebuffer(
                    self.shared_framebuffer,
                    self.backend.framebuffer(),
                );
                if matches!(&response, EmuResponse::GuestCallCompleted { .. }) {
                    effects.potentially_dirty = true;
                    self.mark_audio_discontinuity(
                        crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation,
                    );
                }
                effects.response = Some(response);
            }
            EmuCommand::UndoGuestCall(state) => {
                let response = if self.backend.supports_guest_calls() {
                    match self.backend.load_state_from_bytes(state) {
                        Ok(()) => EmuResponse::GuestCallUndone,
                        Err(error) => EmuResponse::GuestCallUndoFailed(error.to_string()),
                    }
                } else {
                    EmuResponse::GuestCallUndoFailed(
                        "guest calls are not supported by this core".to_string(),
                    )
                };
                super::types::publish_framebuffer(
                    self.shared_framebuffer,
                    self.backend.framebuffer(),
                );
                if matches!(&response, EmuResponse::GuestCallUndone) {
                    self.backend.discard_game_boy_printer_jobs();
                    effects.potentially_dirty = true;
                    self.mark_audio_discontinuity(
                        crate::audio_recorder::AudioTimelineDiscontinuity::GuestCallUndo,
                    );
                }
                effects.response = Some(response);
            }
            EmuCommand::CaptureReplayCheckpoint { frame } => {
                effects.response = Some(if self.backend.supports_replay() {
                    match EmuThread::encode_current_state(self.backend) {
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
                });
            }
            EmuCommand::SetGameBoySerialDevice(device) => {
                self.backend.set_game_boy_serial_device(device);
            }
            EmuCommand::QueueBardigunBarcodeScan(bytes) => {
                let byte_count = bytes.len();
                effects.response = Some(match self.backend.queue_bardigun_barcode_scan(bytes) {
                    Ok(()) => EmuResponse::BardigunBarcodeScanStarted(byte_count),
                    Err(error) => EmuResponse::BardigunBarcodeScanFailed(error.to_string()),
                });
            }
            EmuCommand::TriggerBarcodeBoyScan(digits) => {
                effects.response = Some(match self.backend.trigger_barcode_boy_scan(&digits) {
                    Ok(()) => EmuResponse::BarcodeBoyScanStarted,
                    Err(error) => EmuResponse::BarcodeBoyScanFailed(error.to_string()),
                });
            }
            EmuCommand::RestoreGameBoyLinkState(state) => {
                self.backend.restore_game_boy_link_replay_state(state);
            }
            EmuCommand::UpdateCheats(cheats) => {
                if self.backend.supports_cheats() {
                    *self.last_cheats = cheats;
                    self.backend.install_rom_patches(self.last_cheats);
                } else {
                    self.last_cheats.clear();
                }
            }
            EmuCommand::Rewind(steps) => {
                let response = EmuThread::handle_rewind(
                    self.backend,
                    self.rewind_buffer,
                    self.shared_framebuffer,
                    steps,
                );
                if matches!(&response, EmuResponse::RewindOk { .. }) {
                    effects.potentially_dirty = true;
                    self.backend.discard_game_boy_printer_jobs();
                    self.backend.install_rom_patches(self.last_cheats);
                    self.mark_audio_discontinuity(
                        crate::audio_recorder::AudioTimelineDiscontinuity::Rewind,
                    );
                }
                effects.response = Some(response);
            }
            EmuCommand::Reset => {
                self.backend.reset();
                self.backend.install_rom_patches(self.last_cheats);
                self.rewind_buffer.clear();
                *self.runtime_fault = WorkerRuntimeFault::default();
                self.mark_audio_discontinuity(
                    crate::audio_recorder::AudioTimelineDiscontinuity::Reset,
                );
                effects.potentially_dirty = true;
            }
            platform_specific => {
                return CommonCommandDispatch::PlatformSpecific(platform_specific);
            }
        }

        CommonCommandDispatch::Handled(effects)
    }

    fn mark_audio_discontinuity(
        &mut self,
        reason: crate::audio_recorder::AudioTimelineDiscontinuity,
    ) {
        mark_audio_discontinuity(
            *self.audio_recording_capture,
            self.pending_audio_discontinuities,
            reason,
        );
    }
}

pub(super) fn capture_replay_start_response(
    backend: &EmuBackend,
    capture_id: u64,
    blocker: Option<String>,
    metadata: impl FnOnce() -> zeff_emu_common::replay::ReplayMetadata,
) -> EmuResponse {
    if !backend.supports_replay() {
        return EmuResponse::ReplayStartCaptureFailed {
            capture_id,
            error: "replay capture is not supported by this core".to_string(),
        };
    }
    if let Some(blocker) = blocker {
        return EmuResponse::ReplayStartCaptureFailed {
            capture_id,
            error: format!("replay start rejected: {blocker}"),
        };
    }

    let metadata = metadata();
    match backend.encode_replay_start_state_bytes() {
        Ok(bytes) => EmuResponse::ReplayStartCaptured {
            capture_id,
            start: Box::new(super::ReplayStartState {
                state_bytes: bytes,
                frame_count: backend.frame_count(),
                game_boy_cpu_cycles: backend.game_boy_cpu_cycles(),
                wonder_swan_cpu_cycles: backend.wonder_swan_cpu_cycles(),
                metadata,
            }),
        },
        Err(error) => EmuResponse::ReplayStartCaptureFailed {
            capture_id,
            error: error.to_string(),
        },
    }
}
