mod checkpoint;
pub use checkpoint::InstructionOriginCheckpoint;
use core::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct InstructionByteOrigin(u64);

impl InstructionByteOrigin {
    pub const UNKNOWN: Self = Self(0);
    pub const NON_STORAGE: Self = Self(1);
    pub const MAX_STORAGE_OFFSET: u64 = u64::MAX - 2;

    pub const fn storage(offset: u64) -> Option<Self> {
        match offset.checked_add(2) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn storage_offset(self) -> Option<u64> {
        self.0.checked_sub(2)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstructionInput {
    pub value: u16,
    pub origin: [InstructionByteOrigin; 2],
}

impl InstructionInput {
    pub const fn unknown(value: u16) -> Self {
        Self {
            value,
            origin: [InstructionByteOrigin::UNKNOWN; 2],
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Pipeline {
    pub(super) address: u32,
    pub(super) words: [u16; 2],
    origin: [[InstructionByteOrigin; 2]; 2],
}

impl fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let full = f.alternate();
        let mut pipeline = f.debug_struct("Pipeline");
        pipeline
            .field("address", &self.address)
            .field("words", &self.words);
        if full || self.origin != [[InstructionByteOrigin::UNKNOWN; 2]; 2] {
            pipeline.field("origin", &self.origin);
        }
        pipeline.finish()
    }
}

impl PartialEq for Pipeline {
    fn eq(&self, other: &Self) -> bool {
        self.address == other.address && self.words == other.words
    }
}

impl Eq for Pipeline {}

impl Pipeline {
    pub(super) fn sampled(address: u32, words: [InstructionInput; 2]) -> Self {
        Self {
            address,
            words: words.map(|word| word.value),
            origin: words.map(|word| word.origin),
        }
    }

    pub(super) fn unknown(address: u32, words: [u16; 2]) -> Self {
        Self::sampled(address, words.map(InstructionInput::unknown))
    }

    pub(super) fn input(self, index: usize) -> InstructionInput {
        InstructionInput {
            value: self.words[index],
            origin: self.origin[index],
        }
    }
}

pub(super) fn unknown_cycle<E>(
    cycle: super::CycleResult<u16, E>,
) -> super::CycleResult<InstructionInput, E> {
    super::CycleResult {
        result: cycle.result.map(InstructionInput::unknown),
        wait_clocks: cycle.wait_clocks,
    }
}

pub(super) fn unknown_observed<E>(
    observed: super::ObservedCycle<u16, E>,
) -> super::ObservedCycle<InstructionInput, E> {
    super::ObservedCycle {
        cycle: unknown_cycle(observed.cycle),
        samples: observed.samples,
    }
}

pub(super) fn unknown_entry<E>(
    report: super::physical::EntryReport<u16, E>,
) -> super::physical::EntryReport<InstructionInput, E> {
    use super::physical::{EntryOutcome, EntryReport};
    EntryReport {
        elapsed: report.elapsed,
        outcome: match report.outcome {
            EntryOutcome::Complete(value) => {
                EntryOutcome::Complete(InstructionInput::unknown(value))
            }
            EntryOutcome::HostFailure(error) => EntryOutcome::HostFailure(error),
            EntryOutcome::CriticalAbortReleased => EntryOutcome::CriticalAbortReleased,
            EntryOutcome::TimingUnavailable => EntryOutcome::TimingUnavailable,
        },
    }
}

pub(super) fn unknown_physical<E>(
    report: super::physical_instruction::InstructionReport<u16, E>,
) -> super::physical_instruction::InstructionReport<InstructionInput, E> {
    use super::physical_instruction::{InstructionOutcome, InstructionReport};
    InstructionReport {
        elapsed: report.elapsed,
        outcome: match report.outcome {
            InstructionOutcome::Complete(value) => {
                InstructionOutcome::Complete(InstructionInput::unknown(value))
            }
            InstructionOutcome::HostFailure(error) => InstructionOutcome::HostFailure(error),
            InstructionOutcome::AtGroup0Entry => InstructionOutcome::AtGroup0Entry,
            InstructionOutcome::TimingUnavailable => InstructionOutcome::TimingUnavailable,
        },
    }
}

#[cfg(test)]
mod tests;
