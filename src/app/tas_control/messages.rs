use crate::emu_thread::{
    TasControlAcquireRejectedReason, TasExecutionRejectedReason, TasFrameAdvanceRejectedReason,
};

pub(super) fn acquire_rejection_message(reason: TasControlAcquireRejectedReason) -> &'static str {
    match reason {
        TasControlAcquireRejectedReason::AlreadyLeased { .. } => {
            "The loaded game is already controlled by another live run"
        }
        TasControlAcquireRejectedReason::UncappedExecution => {
            "Turn off uncapped speed before starting a live run"
        }
        TasControlAcquireRejectedReason::AudioRecordingActive => {
            "Stop audio recording before starting a live run"
        }
        TasControlAcquireRejectedReason::LinkActivity => {
            "Disconnect the active link before starting a live run"
        }
        TasControlAcquireRejectedReason::PendingFrameDelivery => {
            "The emulator is still finishing a frame; try again"
        }
        TasControlAcquireRejectedReason::RuntimeFault => {
            "The emulator must recover before starting a live run"
        }
        TasControlAcquireRejectedReason::ReplayActivityUnwitnessed => {
            "This emulator session has replay activity that cannot be verified"
        }
        TasControlAcquireRejectedReason::StateWitnessUnavailable => {
            "The emulator could not capture a safe return point"
        }
        TasControlAcquireRejectedReason::LeaseIdExhausted => {
            "The emulator cannot create another live-run lease"
        }
        TasControlAcquireRejectedReason::UnsupportedSystem => {
            "Load the NES or Game Boy game identified by this TAS project"
        }
        TasControlAcquireRejectedReason::IdentityMetadataMismatch => {
            "The running core identity does not match the loaded game"
        }
        TasControlAcquireRejectedReason::LoadProvenanceUnavailable => {
            "Reload the game directly before starting a live run"
        }
        TasControlAcquireRejectedReason::DirectNesFileRequired => {
            "Load the NES cartridge directly from its .nes file"
        }
        TasControlAcquireRejectedReason::SourceMediaMismatch => {
            "The loaded cartridge was modified after it was read"
        }
        TasControlAcquireRejectedReason::ModsEnabledOrApplied => {
            "Disable ROM mods and reload the cartridge before starting a live run"
        }
        TasControlAcquireRejectedReason::PersistentStateNotAbsent => {
            "This live profile requires a cartridge with no loaded battery save"
        }
        TasControlAcquireRejectedReason::NonNeutralInitialInput => {
            "Release all controls and reload the cartridge before starting a live run"
        }
        TasControlAcquireRejectedReason::NonDefaultSampleRate => {
            "Set audio output to 48000 Hz and reload the cartridge"
        }
        TasControlAcquireRejectedReason::FirmwarePresent => {
            "This direct cartridge live profile does not support external firmware"
        }
        TasControlAcquireRejectedReason::NonStandardConsoleHardware => {
            "This live profile supports standard home NES cartridges only"
        }
        TasControlAcquireRejectedReason::NonStandardControllerTopology => {
            "Use standard NES controllers or the built-in NES Zapper before starting a live run"
        }
        TasControlAcquireRejectedReason::RemovableMediaPresent => {
            "Remove mounted disk media before starting a cartridge live run"
        }
        TasControlAcquireRejectedReason::CheatsPresent => {
            "Disable cheats before starting a live run"
        }
    }
}

pub(super) fn execution_rejection_message(reason: TasExecutionRejectedReason) -> &'static str {
    match reason {
        TasExecutionRejectedReason::NoActiveLease
        | TasExecutionRejectedReason::WrongLease { .. }
        | TasExecutionRejectedReason::WrongExecutionProfile { .. }
        | TasExecutionRejectedReason::RunAlreadyAttempted { .. } => {
            "The live run lost its emulator authority"
        }
        TasExecutionRejectedReason::InvalidRunId => "The live run identifier is invalid",
        TasExecutionRejectedReason::InvalidCacheProof => {
            "The selected TAS boundary no longer has a valid execution proof"
        }
        TasExecutionRejectedReason::FrameLimitExceeded => {
            "The selected TAS row exceeds the live-run frame limit"
        }
        TasExecutionRejectedReason::StartStateTooLarge
        | TasExecutionRejectedReason::InvalidStartState
        | TasExecutionRejectedReason::StartStateRestoreFailed => {
            "The TAS starting state could not be restored"
        }
        TasExecutionRejectedReason::NonStandardControllerTopology => {
            "The TAS starting state does not use standard NES controllers"
        }
        TasExecutionRejectedReason::FrameCountOverflow
        | TasExecutionRejectedReason::FrameProgressFailed
        | TasExecutionRejectedReason::StateFrameMismatch => {
            "The live run did not reach the selected TAS row exactly"
        }
        TasExecutionRejectedReason::RuntimeFault => {
            "The emulator stopped while running the selected TAS input"
        }
        TasExecutionRejectedReason::StateCaptureFailed => {
            "The staged game state could not be verified"
        }
        TasExecutionRejectedReason::InvalidInput => {
            "The selected TAS input is outside the active live profile"
        }
    }
}

pub(super) fn frame_advance_rejection_message(
    reason: TasFrameAdvanceRejectedReason,
) -> &'static str {
    match reason {
        TasFrameAdvanceRejectedReason::NoActiveLease
        | TasFrameAdvanceRejectedReason::WrongLease { .. }
        | TasFrameAdvanceRejectedReason::WrongExecutionProfile { .. } => {
            "The live input advance lost its emulator authority"
        }
        TasFrameAdvanceRejectedReason::NoCompletedExecution
        | TasFrameAdvanceRejectedReason::WrongRun { .. }
        | TasFrameAdvanceRejectedReason::InvalidAdvanceId
        | TasFrameAdvanceRejectedReason::UnexpectedAdvanceId { .. }
        | TasFrameAdvanceRejectedReason::AdvanceIdExhausted
        | TasFrameAdvanceRejectedReason::UnexpectedSegmentId { .. }
        | TasFrameAdvanceRejectedReason::SegmentIdExhausted
        | TasFrameAdvanceRejectedReason::SegmentProofMismatch
        | TasFrameAdvanceRejectedReason::CandidateProofMismatch => {
            "The live input advance no longer matches the staged game"
        }
        TasFrameAdvanceRejectedReason::FrameLimitExceeded => {
            "The live input advance reached the supported frame limit"
        }
        TasFrameAdvanceRejectedReason::StateVerificationUnavailable
        | TasFrameAdvanceRejectedReason::CandidateStateDigestMismatch
        | TasFrameAdvanceRejectedReason::CandidateFrameCountMismatch
        | TasFrameAdvanceRejectedReason::StateCaptureFailed => {
            "The advanced game state could not be verified"
        }
        TasFrameAdvanceRejectedReason::FrameCountOverflow
        | TasFrameAdvanceRejectedReason::FrameProgressFailed
        | TasFrameAdvanceRejectedReason::StateFrameMismatch => {
            "The live input did not advance exactly one frame"
        }
        TasFrameAdvanceRejectedReason::RuntimeFault => {
            "The emulator stopped while recording the live input"
        }
        TasFrameAdvanceRejectedReason::InvalidInput => {
            "The live input is outside the active TAS profile"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_rejections_remain_actionable_and_distinct() {
        let zapper = acquire_rejection_message(
            TasControlAcquireRejectedReason::NonStandardControllerTopology,
        );
        let mods = acquire_rejection_message(TasControlAcquireRejectedReason::ModsEnabledOrApplied);
        let sample_rate =
            acquire_rejection_message(TasControlAcquireRejectedReason::NonDefaultSampleRate);

        assert!(zapper.contains("Zapper"));
        assert!(mods.contains("mods"));
        assert!(sample_rate.contains("48000 Hz"));
        assert_ne!(zapper, mods);
        assert_ne!(mods, sample_rate);
    }
}
