use std::fmt;

use super::App;
use crate::emu_thread::{EmuCommand, EmuCommandAuthority, TasControlCommandKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum EmuCommandSendError {
    Denied(TasControlCommandKind),
    NoWorker,
    ChannelClosed,
}

impl fmt::Display for EmuCommandSendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied(kind) => write!(
                formatter,
                "emulator command {kind:?} is denied while TAS control is active"
            ),
            Self::NoWorker => formatter.write_str("no emulator worker is available"),
            Self::ChannelClosed => formatter.write_str("emulator command channel is closed"),
        }
    }
}

impl std::error::Error for EmuCommandSendError {}

pub(in crate::app) fn after_emu_command_preflight<T>(
    preflight: Result<(), EmuCommandSendError>,
    operation: impl FnOnce() -> T,
) -> Result<T, EmuCommandSendError> {
    preflight?;
    Ok(operation())
}

fn preflight_authority(
    classification: EmuCommandAuthority,
    gameplay_allowed: bool,
) -> Result<(), EmuCommandSendError> {
    if let EmuCommandAuthority::Gameplay(kind) = classification
        && !gameplay_allowed
    {
        return Err(EmuCommandSendError::Denied(kind));
    }
    Ok(())
}

fn preflight_send(
    classification: EmuCommandAuthority,
    gameplay_allowed: bool,
    worker_available: bool,
) -> Result<(), EmuCommandSendError> {
    preflight_authority(classification, gameplay_allowed)?;
    worker_available
        .then_some(())
        .ok_or(EmuCommandSendError::NoWorker)
}

impl App {
    pub(in crate::app) fn preflight_emu_command(
        &self,
        command: &EmuCommand,
    ) -> Result<(), EmuCommandSendError> {
        #[cfg(not(target_arch = "wasm32"))]
        let gameplay_allowed = self.worker_gameplay_commands_allowed();
        #[cfg(target_arch = "wasm32")]
        let gameplay_allowed = true;
        preflight_authority(command.authority_classification(), gameplay_allowed)
    }

    pub(in crate::app) fn preflight_emu_command_kind(
        &self,
        kind: TasControlCommandKind,
    ) -> Result<(), EmuCommandSendError> {
        #[cfg(not(target_arch = "wasm32"))]
        let gameplay_allowed = self.worker_gameplay_commands_allowed();
        #[cfg(target_arch = "wasm32")]
        let gameplay_allowed = true;
        preflight_authority(EmuCommandAuthority::Gameplay(kind), gameplay_allowed)
    }

    pub(in crate::app) fn send_emu_command_checked(
        &mut self,
        command: EmuCommand,
    ) -> Result<(), EmuCommandSendError> {
        let authority = command.authority_classification();
        #[cfg(not(target_arch = "wasm32"))]
        let invalidates_readiness = invalidates_tas_readiness(&command);
        #[cfg(not(target_arch = "wasm32"))]
        let gameplay_allowed = self.worker_gameplay_commands_allowed();
        #[cfg(target_arch = "wasm32")]
        let gameplay_allowed = true;
        let preflight = preflight_send(authority, gameplay_allowed, self.emu_thread.is_some());
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(error) = preflight {
            if error == EmuCommandSendError::NoWorker {
                self.terminalize_tas_control_command_loss();
            }
            return Err(error);
        }
        #[cfg(target_arch = "wasm32")]
        preflight?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let sent = self
                .emu_thread
                .as_ref()
                .is_some_and(|thread| thread.send_checked(command));
            if !sent {
                self.terminalize_tas_control_command_loss();
                return Err(EmuCommandSendError::ChannelClosed);
            }
        }
        #[cfg(target_arch = "wasm32")]
        self.emu_thread
            .as_ref()
            .ok_or(EmuCommandSendError::NoWorker)?
            .send(command);

        #[cfg(not(target_arch = "wasm32"))]
        if invalidates_readiness {
            self.tas_control.clear_readiness();
        }

        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn invalidates_tas_readiness(command: &EmuCommand) -> bool {
    matches!(
        command,
        EmuCommand::LoadStateSlot { .. }
            | EmuCommand::LoadStateFromPath { .. }
            | EmuCommand::InspectRecovery { resume: true, .. }
            | EmuCommand::LoadStateBytes { .. }
            | EmuCommand::SetSampleRate(_)
            | EmuCommand::ApplyMediaEvent(_)
            | EmuCommand::SetGameBoySerialDevice(_)
            | EmuCommand::QueueBardigunBarcodeScan(_)
            | EmuCommand::TriggerBarcodeBoyScan(_)
            | EmuCommand::RestoreGameBoyLinkState(_)
            | EmuCommand::UpdateCheats(_)
            | EmuCommand::Reset
            | EmuCommand::StartTcpLink(_)
            | EmuCommand::DisconnectLink
            | EmuCommand::Rewind(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameplay_commands_are_denied_only_while_authority_is_fenced() {
        for kind in [
            TasControlCommandKind::FrameExecution,
            TasControlCommandKind::AudioOrTimingConfiguration,
            TasControlCommandKind::StateOrRecovery,
            TasControlCommandKind::Replay,
            TasControlCommandKind::DebuggerMutation,
            TasControlCommandKind::MediaOrPeripheral,
            TasControlCommandKind::CheatConfiguration,
            TasControlCommandKind::Reset,
            TasControlCommandKind::Link,
            TasControlCommandKind::Rewind,
        ] {
            let classification = EmuCommandAuthority::Gameplay(kind);
            assert_eq!(
                preflight_authority(classification, false),
                Err(EmuCommandSendError::Denied(kind))
            );
            assert_eq!(preflight_authority(classification, true), Ok(()));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn readiness_invalidation_tracks_profile_mutations_only() {
        assert!(invalidates_tas_readiness(&EmuCommand::SetSampleRate(
            48_000
        )));
        assert!(invalidates_tas_readiness(&EmuCommand::UpdateCheats(
            Vec::new()
        )));
        assert!(!invalidates_tas_readiness(
            &EmuCommand::SetUncappedBatchSize(64)
        ));
        assert!(!invalidates_tas_readiness(&EmuCommand::CaptureStateBytes));
    }

    #[test]
    fn authority_transitions_are_the_only_fenced_exceptions() {
        for classification in [
            EmuCommandAuthority::ObserveTasReadiness,
            EmuCommandAuthority::AcquireTasControl,
            EmuCommandAuthority::ExecuteTasControl,
            EmuCommandAuthority::AdvanceTasControl,
            EmuCommandAuthority::RollbackTasControl,
            EmuCommandAuthority::CommitTasControl,
            EmuCommandAuthority::Shutdown,
        ] {
            assert_eq!(preflight_authority(classification, false), Ok(()));
        }
    }

    #[test]
    fn authority_preflight_does_not_require_a_worker() {
        assert_eq!(
            preflight_authority(
                EmuCommandAuthority::Gameplay(TasControlCommandKind::Reset),
                true,
            ),
            Ok(())
        );
    }

    #[test]
    fn send_preflight_types_missing_worker_after_authority_check() {
        assert_eq!(
            preflight_send(
                EmuCommandAuthority::Gameplay(TasControlCommandKind::Reset),
                true,
                false,
            ),
            Err(EmuCommandSendError::NoWorker)
        );
        assert_eq!(
            preflight_send(
                EmuCommandAuthority::Gameplay(TasControlCommandKind::Reset),
                false,
                false,
            ),
            Err(EmuCommandSendError::Denied(TasControlCommandKind::Reset))
        );
    }

    #[test]
    fn command_classifier_maps_representative_commands_and_transitions() {
        let cases = [
            (
                EmuCommand::SetSampleRate(48_000),
                EmuCommandAuthority::Gameplay(TasControlCommandKind::AudioOrTimingConfiguration),
            ),
            (
                EmuCommand::SaveStateSlot(2),
                EmuCommandAuthority::Gameplay(TasControlCommandKind::StateOrRecovery),
            ),
            (
                EmuCommand::CaptureReplayCheckpoint { frame: 3 },
                EmuCommandAuthority::Gameplay(TasControlCommandKind::Replay),
            ),
            (
                EmuCommand::UndoGuestCall(Vec::new()),
                EmuCommandAuthority::Gameplay(TasControlCommandKind::DebuggerMutation),
            ),
            (
                EmuCommand::TriggerBarcodeBoyScan(String::new()),
                EmuCommandAuthority::Gameplay(TasControlCommandKind::MediaOrPeripheral),
            ),
            (
                EmuCommand::UpdateCheats(Vec::new()),
                EmuCommandAuthority::Gameplay(TasControlCommandKind::CheatConfiguration),
            ),
            (
                EmuCommand::Reset,
                EmuCommandAuthority::Gameplay(TasControlCommandKind::Reset),
            ),
            (
                EmuCommand::Rewind(1),
                EmuCommandAuthority::Gameplay(TasControlCommandKind::Rewind),
            ),
            (EmuCommand::Shutdown, EmuCommandAuthority::Shutdown),
        ];

        for (command, expected) in cases {
            assert_eq!(command.authority_classification(), expected);
        }

        #[cfg(not(target_arch = "wasm32"))]
        for (command, expected) in [
            (
                EmuCommand::InspectTasReadiness {
                    request_id: 3,
                    profile: crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
                },
                EmuCommandAuthority::ObserveTasReadiness,
            ),
            (
                EmuCommand::DisconnectLink,
                EmuCommandAuthority::Gameplay(TasControlCommandKind::Link),
            ),
            (
                EmuCommand::AcquireTasControl {
                    request_id: 4,
                    profile: crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
                },
                EmuCommandAuthority::AcquireTasControl,
            ),
            (
                EmuCommand::ExecuteTasControl(Box::new(crate::emu_thread::TasExecutionRequest {
                    profile: crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
                    lease_id: 5,
                    run_id: 6,
                    intermediate_cache_proofs: Vec::new(),
                    cache_proof: crate::emu_thread::TasExecutionCacheProof {
                        sync_identity_sha256: crate::tas_project::TasDigest([0; 32]),
                        branch_prefix_sha256: crate::tas_project::TasDigest([0; 32]),
                        target_cursor: 0,
                    },
                    predecessor_window: None,
                    start_state_bytes: Vec::new(),
                    input_prefix: Vec::new(),
                })),
                EmuCommandAuthority::ExecuteTasControl,
            ),
            (
                EmuCommand::AdvanceTasControl(Box::new(
                    crate::emu_thread::TasFrameAdvanceRequest {
                        profile: crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
                        lease_id: 5,
                        run_id: 6,
                        advance_id: 1,
                        segment_id: 1,
                        expected_segment_frame_count: 1,
                        expected_executed_project_frames: 1,
                        expected_frame_count: 7,
                        expected_state_sha256: crate::tas_project::TasDigest([0; 32]),
                        input: crate::emu_thread::TasInputFrame::default(),
                        snapshot: None,
                    },
                )),
                EmuCommandAuthority::AdvanceTasControl,
            ),
            (
                EmuCommand::RollbackTasControl { lease_id: 5 },
                EmuCommandAuthority::RollbackTasControl,
            ),
            (
                EmuCommand::CommitTasControl { lease_id: 6 },
                EmuCommandAuthority::CommitTasControl,
            ),
        ] {
            assert_eq!(command.authority_classification(), expected);
        }
    }

    #[test]
    fn denied_preflight_does_not_run_caller_side_effects() {
        let mut called = false;
        let result = after_emu_command_preflight(
            Err(EmuCommandSendError::Denied(
                TasControlCommandKind::StateOrRecovery,
            )),
            || called = true,
        );

        assert_eq!(
            result,
            Err(EmuCommandSendError::Denied(
                TasControlCommandKind::StateOrRecovery
            ))
        );
        assert!(!called);
    }
}
