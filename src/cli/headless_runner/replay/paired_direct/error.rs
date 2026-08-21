use std::fmt;

use zeff_emu_common::replay::{ReplayGameBoyLinkAction, ReplayGameBoyLinkReply};
use zeff_gb_core::hardware::bus::{GameBoyLinkAction, GameBoyLinkReply};

use super::super::paired_plan::{Point, Side};

#[derive(Debug)]
pub(in crate::cli::headless_runner::replay) enum DirectCoordinatorError {
    UnsupportedStateOrEvent {
        side: Side,
        ordinal: usize,
    },
    UnsupportedCrossedBatch {
        transfers: usize,
    },
    InvalidCrossedBatch,
    MissingStartTick {
        side: Side,
    },
    StateDuringPreparedTransfer {
        side: Side,
        id: u64,
        ordinal: usize,
    },
    UnsafeStatePayload {
        side: Side,
        ordinal: usize,
    },
    UnsafeStartState {
        side: Side,
    },
    ConflictingPassiveStartStates,
    StateNotAtFrameBoundary {
        side: Side,
        ordinal: usize,
    },
    StateTickOverflow {
        side: Side,
        ordinal: usize,
    },
    StateOverwritesTransfer {
        side: Side,
        ordinal: usize,
        tick: u64,
    },
    TransferTickOverflow {
        side: Side,
    },
    ReplyObservationRequiresStep {
        side: Side,
        frame: u64,
    },
    DelayedReply {
        side: Side,
        id: u64,
        start: Point,
        reply: Point,
    },
    FrameOverflow {
        side: Side,
    },
    FrameOrder {
        side: Side,
        expected: usize,
        actual: usize,
    },
    FrameCommitBlocked {
        side: Side,
    },
    Overshot {
        side: Side,
        target: u64,
        actual: u64,
    },
    EarlyBoundary {
        side: Side,
    },
    NoProgress {
        side: Side,
    },
    Suspended {
        side: Side,
    },
    UnexpectedAction {
        side: Side,
    },
    ActionMismatch {
        side: Side,
        expected: ReplayGameBoyLinkAction,
        actual: Option<GameBoyLinkAction>,
    },
    ReplyMismatch {
        side: Side,
        expected: ReplayGameBoyLinkReply,
        actual: GameBoyLinkReply,
    },
    MissingPreparedToken {
        side: Side,
    },
    UnexpectedPreparedToken {
        side: Side,
    },
    IncompatibleBackends,
    GeneratedOrder {
        side: Side,
    },
    Checkpoint {
        side: Side,
        message: String,
    },
    Exchange(String),
    Backend(String),
}

impl fmt::Display for DirectCoordinatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedStateOrEvent { side, ordinal } => write!(
                f,
                "direct paired replay does not yet support {side} non-semantic event at ordinal {ordinal}"
            ),
            Self::UnsupportedCrossedBatch { transfers } => write!(
                f,
                "direct paired replay does not yet support a crossed batch of {transfers} transfers"
            ),
            Self::InvalidCrossedBatch => {
                f.write_str("direct paired replay crossed batch does not have one master per side")
            }
            Self::MissingStartTick { side } => {
                write!(
                    f,
                    "direct paired replay is missing the {side} link start tick"
                )
            }
            Self::StateDuringPreparedTransfer { side, id, ordinal } => write!(
                f,
                "{side} replay state event at ordinal {ordinal} would split prepared transfer {id:#018X}"
            ),
            Self::UnsafeStatePayload { side, ordinal } => write!(
                f,
                "{side} replay state event at ordinal {ordinal} contains an owned transfer"
            ),
            Self::UnsafeStartState { side } => {
                write!(
                    f,
                    "{side} replay link start state contains an owned transfer"
                )
            }
            Self::ConflictingPassiveStartStates => {
                f.write_str("both paired GB replay endpoints contain passive in-flight start state")
            }
            Self::StateNotAtFrameBoundary { side, ordinal } => write!(
                f,
                "{side} replay frame state at ordinal {ordinal} is not at an idle frame boundary"
            ),
            Self::StateTickOverflow { side, ordinal } => write!(
                f,
                "{side} replay state event at ordinal {ordinal} overflows its absolute tick"
            ),
            Self::StateOverwritesTransfer {
                side,
                ordinal,
                tick,
            } => write!(
                f,
                "{side} replay state event at ordinal {ordinal} would overwrite an in-flight transfer at tick {tick}"
            ),
            Self::TransferTickOverflow { side } => {
                write!(f, "{side} replay transfer completion tick overflowed")
            }
            Self::ReplyObservationRequiresStep { side, frame } => write!(
                f,
                "{side} replay reply observation at frame {frame} would require advancing a prepared master"
            ),
            Self::DelayedReply {
                side,
                id,
                start,
                reply,
            } => write!(
                f,
                "direct paired replay does not support {side} transfer {id:#018X} reply observation: start={start:?} reply={reply:?}"
            ),
            Self::FrameOverflow { side } => write!(f, "{side} replay frame count overflows usize"),
            Self::FrameOrder {
                side,
                expected,
                actual,
            } => write!(
                f,
                "{side} replay frame order diverged: expected {expected}, got {actual}"
            ),
            Self::FrameCommitBlocked { side } => {
                write!(
                    f,
                    "{side} replay point crosses an uncommitted completed frame"
                )
            }
            Self::Overshot {
                side,
                target,
                actual,
            } => write!(
                f,
                "{side} replay overshot exact tick {target} at instruction boundary {actual}"
            ),
            Self::EarlyBoundary { side } => {
                write!(f, "{side} replay reached a link boundary before its target")
            }
            Self::NoProgress { side } => write!(f, "{side} replay made no typed frame progress"),
            Self::Suspended { side } => write!(f, "{side} replay side suspended"),
            Self::UnexpectedAction { side } => {
                write!(f, "{side} replay produced an unexpected local link action")
            }
            Self::ActionMismatch {
                side,
                expected,
                actual,
            } => write!(
                f,
                "{side} replay link action mismatch: expected {expected:?}, got {actual:?}"
            ),
            Self::ReplyMismatch {
                side,
                expected,
                actual,
            } => write!(
                f,
                "{side} replay link reply mismatch: expected {expected:?}, got {actual:?}"
            ),
            Self::MissingPreparedToken { side } => {
                write!(f, "{side} replay exchange did not prepare its master token")
            }
            Self::UnexpectedPreparedToken { side } => {
                write!(
                    f,
                    "{side} replay exchange prepared an unexpected master token"
                )
            }
            Self::IncompatibleBackends => {
                f.write_str("direct paired replay requires two Game Boy backends")
            }
            Self::GeneratedOrder { side } => {
                write!(
                    f,
                    "{side} replay generated semantic events out of source order"
                )
            }
            Self::Checkpoint { side, message } => {
                write!(f, "{side} replay checkpoint mismatch: {message}")
            }
            Self::Exchange(message) => write!(f, "direct GB exchange failed: {message}"),
            Self::Backend(message) => write!(f, "direct GB replay backend failed: {message}"),
        }
    }
}

impl std::error::Error for DirectCoordinatorError {}
