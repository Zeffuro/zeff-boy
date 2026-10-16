//! Imported group-0 boundaries and a selected warm-reset profile only.
//! No instruction BERR capture, abort overhead or physical partial-register model.

use super::{CpuClocks, Pipeline, PrefetchCpu};
use crate::Registers;
use crate::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use crate::constants::{
    BUS_ADDRESS_MASK, CCR_MASK, ENTRY_REFILL_IDLE_CLOCKS, GROUP0_FRAME_BYTES,
    GROUP0_INITIAL_CLOCKS, RESET_INITIAL_CLOCKS, RESET_PC_VECTOR_ADDRESS, RESET_SSP_VECTOR_ADDRESS,
    RESET_STATUS, SSW_DEFINED_MASK, SSW_NOT_INSTRUCTION, SSW_READ, STATUS_MASK, SUPERVISOR, TRACE,
    TRANSFER_CLOCKS, VECTOR_ADDRESS_ERROR, VECTOR_BUS_ERROR, VECTOR_ENTRY_BYTES,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryOutcome<T, E> {
    Complete(T),
    HostFailure(E),
    CriticalAbortReleased,
    TimingUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryReport<T, E> {
    pub elapsed: u32,
    pub outcome: EntryOutcome<T, E>,
}

/// Callbacks advance devices by TOTAL elapsed, not four clocks plus waits.
/// CriticalAbortReleased ends at a qualified abort-release/halt boundary.
/// TimingUnavailable is a known critical fault with an unqualified terminal boundary.
/// Complete word transfers require at least four clocks; detection has no transfer minimum.
pub trait EntryBus {
    type Error;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, Self::Error>;
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), Self::Error>;
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32);
    /// Detection only: no aligned memory transfer or data strobes. Default supplies no timing.
    fn detect_address_error(
        &mut self,
        _access: BusAccess,
        _direction: Direction,
        _start: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        EntryReport {
            elapsed: 0,
            outcome: EntryOutcome::TimingUnavailable,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessingClass {
    InstructionOrGroup2,
    Group0OrGroup1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SswProfile {
    SelectedIrdUpperBits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group0Capture {
    pub vector: u8,
    pub internal_pc: u32,
    pub phase_sr: u16,
    pub ird: u16,
    pub logical_address: u32,
    pub function_code: FunctionCode,
    pub direction: Direction,
    pub processing_class: ProcessingClass,
    pub ssw_profile: SswProfile,
}

impl Group0Capture {
    fn validate(self) -> Result<(), EntryInputError> {
        if !matches!(self.vector, VECTOR_BUS_ERROR | VECTOR_ADDRESS_ERROR) {
            return Err(EntryInputError::InvalidVector);
        }
        if self.phase_sr & !STATUS_MASK != 0 {
            return Err(EntryInputError::InvalidStatus);
        }
        if self.function_code == FunctionCode::CpuSpace {
            return Err(EntryInputError::UnsupportedFunctionCode);
        }
        Ok(())
    }

    pub fn special_status_word(self) -> u16 {
        let upper = match self.ssw_profile {
            SswProfile::SelectedIrdUpperBits => self.ird & !SSW_DEFINED_MASK,
        };
        upper
            | self.function_code as u16
            | if self.direction == Direction::Read {
                SSW_READ
            } else {
                0
            }
            | if self.processing_class == ProcessingClass::Group0OrGroup1 {
                SSW_NOT_INSTRUCTION
            } else {
                0
            }
    }
}

/// Caller-supplied bser1/aser1 witness, with prior ownership released and abort time included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportedGroup0Boundary {
    pub registers: Registers,
    pub clock: CpuClocks,
    pub capture: Group0Capture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetQualification {
    QualifiedSafeAssertionAndRelease,
    Unqualified,
}

/// Hold time has already advanced devices; sampled release must not precede CPU clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalResetBoundary {
    pub sampled_release: CpuClocks,
    pub qualification: ResetQualification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryInputError {
    InvalidVector,
    InvalidStatus,
    UnsupportedFunctionCode,
    UnqualifiedReset,
    NonmonotonicRelease,
    ChangedImportedSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    ImportedGroup0,
    ExternalWarmReset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CriticalFault {
    BusAbort {
        access: BusAccess,
        direction: Direction,
    },
    AddressError {
        access: BusAccess,
        direction: Direction,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingQualification {
    Qualified,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PhysicalEntryState {
    #[default]
    Idle,
    Critical(EntryKind),
    DoubleFaultHalted {
        cause: CriticalFault,
        timing: TimingQualification,
    },
    FaultUnqualified {
        cause: CriticalFault,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PhysicalInspection {
    #[default]
    DiagnosticCompleteValues,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryFault<E> {
    Input(EntryInputError),
    NoImportedBoundary,
    HostFrozen,
    HostFailure {
        access: BusAccess,
        direction: Direction,
        source: E,
    },
    DoubleFaultHalted(CriticalFault),
    TimingUnavailable(CriticalFault),
    FaultUnqualified(CriticalFault),
    ClockOverflow,
    InvalidCompleteDuration {
        elapsed: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryStep {
    pub kind: EntryKind,
    pub clocks: u64,
}

impl PrefetchCpu {
    pub fn from_group0_boundary(boundary: ImportedGroup0Boundary) -> Result<Self, EntryInputError> {
        boundary.capture.validate()?;
        Ok(Self {
            registers: boundary.registers,
            clock: boundary.clock,
            imported_boundary: Some(boundary),
            ..Self::default()
        })
    }

    pub fn physical_entry_state(&self) -> PhysicalEntryState {
        self.physical_state
    }

    /// Registers retain diagnostic whole values; physical partials are unavailable on abort.
    pub fn physical_inspection(&self) -> PhysicalInspection {
        self.physical_inspection
    }

    pub fn enter_imported_group0<B: EntryBus>(
        &mut self,
        bus: &mut B,
    ) -> Result<EntryStep, EntryFault<B::Error>> {
        self.entry_guard()?;
        let boundary = self
            .imported_boundary
            .ok_or(EntryFault::NoImportedBoundary)?;
        if self.registers != boundary.registers || self.clock != boundary.clock {
            return Err(EntryFault::Input(EntryInputError::ChangedImportedSnapshot));
        }
        self.imported_boundary = None;
        let capture = boundary.capture;
        self.begin_entry(EntryKind::ImportedGroup0);
        let start = self.clock.0;
        let sp = self.registers.ssp;
        self.entry_idle(bus, GROUP0_INITIAL_CLOCKS)?;
        for (offset, value) in [
            (2, capture.internal_pc as u16),
            (6, capture.phase_sr),
            (4, (capture.internal_pc >> 16) as u16),
            (8, capture.ird),
            (10, capture.logical_address as u16),
            (14, capture.special_status_word()),
            (12, (capture.logical_address >> 16) as u16),
        ] {
            self.entry_write(bus, sp.wrapping_sub(offset), value)?;
        }
        let address = u32::from(capture.vector) * VECTOR_ENTRY_BYTES;
        let target = self.entry_long(
            bus,
            address,
            FunctionCode::SupervisorData,
            AccessKind::Vector,
        )?;
        let words = self.entry_refill(bus, target)?;
        self.registers.ssp = sp.wrapping_sub(GROUP0_FRAME_BYTES);
        self.registers
            .set_status_register((capture.phase_sr | SUPERVISOR) & !TRACE);
        self.commit_entry(target, words);
        Ok(EntryStep {
            kind: EntryKind::ImportedGroup0,
            clocks: self.clock.0 - start,
        })
    }

    /// Selected I14/six-read/I2 profile, not a reset-pin timing model. Host freezes persist.
    pub fn external_warm_reset<B: EntryBus>(
        &mut self,
        bus: &mut B,
        boundary: ExternalResetBoundary,
    ) -> Result<EntryStep, EntryFault<B::Error>> {
        if self.frozen {
            return Err(EntryFault::HostFrozen);
        }
        if boundary.qualification != ResetQualification::QualifiedSafeAssertionAndRelease {
            return Err(EntryFault::Input(EntryInputError::UnqualifiedReset));
        }
        if boundary.sampled_release < self.clock {
            return Err(EntryFault::Input(EntryInputError::NonmonotonicRelease));
        }
        self.clock = boundary.sampled_release;
        self.imported_boundary = None;
        self.begin_entry(EntryKind::ExternalWarmReset);
        let start = self.clock.0;
        self.entry_idle(bus, RESET_INITIAL_CLOCKS)?;
        let sp = self.entry_long(
            bus,
            RESET_SSP_VECTOR_ADDRESS,
            FunctionCode::SupervisorProgram,
            AccessKind::ResetVector,
        )?;
        let target = self.entry_long(
            bus,
            RESET_PC_VECTOR_ADDRESS,
            FunctionCode::SupervisorProgram,
            AccessKind::ResetVector,
        )?;
        let words = self.entry_refill(bus, target)?;
        let sr = self.registers.status_register();
        self.registers.ssp = sp;
        self.registers
            .set_status_register((sr & CCR_MASK) | RESET_STATUS);
        self.commit_entry(target, words);
        Ok(EntryStep {
            kind: EntryKind::ExternalWarmReset,
            clocks: self.clock.0 - start,
        })
    }

    fn begin_entry(&mut self, kind: EntryKind) {
        self.pipeline = None;
        self.pending_trace = false;
        self.stopped = false;
        self.interrupts.clear_accepted_edge();
        self.physical_state = PhysicalEntryState::Critical(kind);
    }

    fn commit_entry(&mut self, pc: u32, words: [u16; 2]) {
        self.registers.pc = pc;
        self.pipeline = Some(Pipeline { address: pc, words });
        self.physical_inspection = PhysicalInspection::DiagnosticCompleteValues;
    }

    fn entry_guard<E>(&self) -> Result<(), EntryFault<E>> {
        if self.frozen {
            return Err(EntryFault::HostFrozen);
        }
        if let PhysicalEntryState::DoubleFaultHalted { cause, .. } = self.physical_state {
            return Err(EntryFault::DoubleFaultHalted(cause));
        }
        if let PhysicalEntryState::FaultUnqualified { cause } = self.physical_state {
            return Err(EntryFault::FaultUnqualified(cause));
        }
        Ok(())
    }

    pub(super) fn physically_halted(&self) -> bool {
        matches!(
            self.physical_state,
            PhysicalEntryState::DoubleFaultHalted { .. }
        )
    }

    pub(super) fn execution_guard<E>(&self) -> Result<(), super::TimingFault<E>> {
        if self.frozen {
            return Err(super::TimingFault::Frozen);
        }
        if self.physically_halted() {
            return Err(super::TimingFault::DoubleFaultHalted);
        }
        if matches!(
            self.physical_state,
            PhysicalEntryState::FaultUnqualified { .. }
        ) {
            return Err(super::TimingFault::PhysicalFaultUnqualified);
        }
        if self.imported_boundary.is_some() {
            return Err(super::TimingFault::ImportedBoundaryPending);
        }
        Ok(())
    }

    pub(super) fn entry_idle<B: EntryBus>(
        &mut self,
        bus: &mut B,
        clocks: u32,
    ) -> Result<(), EntryFault<B::Error>> {
        let Some(end) = self.clock.0.checked_add(u64::from(clocks)) else {
            self.frozen = true;
            return Err(EntryFault::ClockOverflow);
        };
        bus.idle_entry(self.clock, clocks);
        self.clock.0 = end;
        Ok(())
    }

    pub(super) fn entry_reserve<E>(&mut self) -> Result<(), EntryFault<E>> {
        if self.clock.0.checked_add(u64::from(u32::MAX)).is_none() {
            self.frozen = true;
            return Err(EntryFault::ClockOverflow);
        }
        Ok(())
    }

    fn entry_halt<E>(
        &mut self,
        cause: CriticalFault,
        timing: TimingQualification,
    ) -> EntryFault<E> {
        self.physical_state = PhysicalEntryState::DoubleFaultHalted { cause, timing };
        self.physical_inspection = PhysicalInspection::Unavailable;
        match timing {
            TimingQualification::Qualified => EntryFault::DoubleFaultHalted(cause),
            TimingQualification::Unavailable => EntryFault::TimingUnavailable(cause),
        }
    }

    fn entry_finish<T, E>(
        &mut self,
        report: EntryReport<T, E>,
        access: BusAccess,
        direction: Direction,
    ) -> Result<T, EntryFault<E>> {
        self.clock.0 += u64::from(report.elapsed);
        match report.outcome {
            EntryOutcome::Complete(value) if report.elapsed >= TRANSFER_CLOCKS => Ok(value),
            EntryOutcome::Complete(_) => {
                self.frozen = true;
                Err(EntryFault::InvalidCompleteDuration {
                    elapsed: report.elapsed,
                })
            }
            EntryOutcome::HostFailure(source) => {
                self.frozen = true;
                Err(EntryFault::HostFailure {
                    access,
                    direction,
                    source,
                })
            }
            EntryOutcome::CriticalAbortReleased => Err(self.entry_halt(
                CriticalFault::BusAbort { access, direction },
                TimingQualification::Qualified,
            )),
            EntryOutcome::TimingUnavailable => Err(self.entry_halt(
                CriticalFault::BusAbort { access, direction },
                TimingQualification::Unavailable,
            )),
        }
    }

    fn entry_access<B: EntryBus>(
        &mut self,
        bus: &mut B,
        address: u32,
        fc: FunctionCode,
        kind: AccessKind,
        direction: Direction,
    ) -> Result<BusAccess, EntryFault<B::Error>> {
        let access = BusAccess {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK & !1,
            function_code: fc,
            kind,
            lanes: ByteLanes::Both,
        };
        self.entry_reserve()?;
        if address & 1 != 0 {
            let report = bus.detect_address_error(access, direction, self.clock);
            self.clock.0 += u64::from(report.elapsed);
            let cause = CriticalFault::AddressError { access, direction };
            return Err(match report.outcome {
                EntryOutcome::HostFailure(source) => {
                    self.frozen = true;
                    EntryFault::HostFailure {
                        access,
                        direction,
                        source,
                    }
                }
                EntryOutcome::TimingUnavailable => {
                    self.entry_halt(cause, TimingQualification::Unavailable)
                }
                EntryOutcome::Complete(()) | EntryOutcome::CriticalAbortReleased => {
                    self.entry_halt(cause, TimingQualification::Qualified)
                }
            });
        }
        Ok(access)
    }

    fn entry_read<B: EntryBus>(
        &mut self,
        bus: &mut B,
        address: u32,
        fc: FunctionCode,
        kind: AccessKind,
    ) -> Result<u16, EntryFault<B::Error>> {
        let access = self.entry_access(bus, address, fc, kind, Direction::Read)?;
        let report = bus.read_entry(access, self.clock);
        self.entry_finish(report, access, Direction::Read)
    }

    fn entry_write<B: EntryBus>(
        &mut self,
        bus: &mut B,
        address: u32,
        value: u16,
    ) -> Result<(), EntryFault<B::Error>> {
        let access = self.entry_access(
            bus,
            address,
            FunctionCode::SupervisorData,
            AccessKind::Stack,
            Direction::Write,
        )?;
        let report = bus.write_entry(access, value, self.clock);
        self.entry_finish(report, access, Direction::Write)
    }

    fn entry_long<B: EntryBus>(
        &mut self,
        bus: &mut B,
        address: u32,
        fc: FunctionCode,
        kind: AccessKind,
    ) -> Result<u32, EntryFault<B::Error>> {
        let high = self.entry_read(bus, address, fc, kind)?;
        let low = self.entry_read(bus, address.wrapping_add(2), fc, kind)?;
        Ok((u32::from(high) << 16) | u32::from(low))
    }

    fn entry_refill<B: EntryBus>(
        &mut self,
        bus: &mut B,
        target: u32,
    ) -> Result<[u16; 2], EntryFault<B::Error>> {
        let first = self.entry_read(
            bus,
            target,
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
        )?;
        self.entry_idle(bus, ENTRY_REFILL_IDLE_CLOCKS)?;
        let second = self.entry_read(
            bus,
            target.wrapping_add(2),
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
        )?;
        Ok([first, second])
    }
}
