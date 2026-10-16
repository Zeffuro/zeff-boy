use super::*;
use crate::bus::{AccessKind, ByteLanes};
use crate::constants::{BUS_ADDRESS_MASK, TRANSFER_CLOCKS, VECTOR_ADDRESS_ERROR, VECTOR_BUS_ERROR};
use crate::timing::physical::{
    ImportedGroup0Boundary, PhysicalInspection, ProcessingClass, SswProfile,
};

pub(super) enum TransferExit<E> {
    Entered(Group0Capture),
    Fault(InstructionFault<E>),
}

#[derive(Clone, Copy)]
pub(super) struct TransferPhase {
    pub phase: InstructionPhase,
    pub access: BusAccess,
    pub direction: Direction,
    pub internal_pc: u32,
    pub ird: u16,
}

impl PrefetchCpu {
    pub(super) fn prefetch_phase(
        &self,
        phase: InstructionPhase,
        address: u32,
        internal_pc: u32,
        ird: u16,
    ) -> TransferPhase {
        TransferPhase {
            phase,
            access: BusAccess {
                logical_address: address,
                bus_address: address & BUS_ADDRESS_MASK & !1,
                function_code: crate::ea::program_space(&self.registers),
                kind: AccessKind::Prefetch,
                lanes: ByteLanes::Both,
            },
            direction: Direction::Read,
            internal_pc,
            ird,
        }
    }

    pub(super) fn physical_reserve<E>(&mut self) -> Result<(), TransferExit<E>> {
        self.entry_reserve()
            .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))
    }

    pub(super) fn physical_read<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        phase: TransferPhase,
    ) -> Result<u16, TransferExit<B::Error>> {
        self.physical_reserve()?;
        self.physical_detect(bus, phase)?;
        let report = bus.read_instruction(phase.access, self.clock);
        self.physical_report(bus, phase, report)
    }

    pub(super) fn physical_write<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        phase: TransferPhase,
        value: u16,
    ) -> Result<(), TransferExit<B::Error>> {
        self.physical_reserve()?;
        self.physical_detect(bus, phase)?;
        let report = bus.write_instruction(phase.access, value, self.clock);
        self.physical_report(bus, phase, report)
    }

    fn physical_detect<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        phase: TransferPhase,
    ) -> Result<(), TransferExit<B::Error>> {
        if phase.access.lanes != ByteLanes::Both || phase.access.logical_address & 1 == 0 {
            return Ok(());
        }
        let report =
            bus.detect_instruction_address_error(phase.access, phase.direction, self.clock);
        self.clock.0 += u64::from(report.elapsed);
        Err(match report.outcome {
            InstructionOutcome::HostFailure(source) => self.physical_host_fault(phase, source),
            InstructionOutcome::TimingUnavailable | InstructionOutcome::Complete(()) => {
                self.physical_unqualified(phase, VECTOR_ADDRESS_ERROR, false)
            }
            InstructionOutcome::AtGroup0Entry => {
                self.physical_group0(bus, phase, VECTOR_ADDRESS_ERROR)
            }
        })
    }

    fn physical_report<B: InstructionBus, T>(
        &mut self,
        bus: &mut B,
        phase: TransferPhase,
        report: InstructionReport<T, B::Error>,
    ) -> Result<T, TransferExit<B::Error>> {
        self.clock.0 += u64::from(report.elapsed);
        match report.outcome {
            InstructionOutcome::Complete(value) if report.elapsed >= TRANSFER_CLOCKS => Ok(value),
            InstructionOutcome::Complete(_) => {
                self.frozen = true;
                Err(TransferExit::Fault(InstructionFault::Entry(
                    EntryFault::InvalidCompleteDuration {
                        elapsed: report.elapsed,
                    },
                )))
            }
            InstructionOutcome::HostFailure(source) => Err(self.physical_host_fault(phase, source)),
            InstructionOutcome::TimingUnavailable => {
                Err(self.physical_unqualified(phase, VECTOR_BUS_ERROR, false))
            }
            InstructionOutcome::AtGroup0Entry => {
                Err(self.physical_group0(bus, phase, VECTOR_BUS_ERROR))
            }
        }
    }

    fn physical_host_fault<E>(&mut self, phase: TransferPhase, source: E) -> TransferExit<E> {
        self.frozen = true;
        self.pipeline = None;
        TransferExit::Fault(InstructionFault::Entry(EntryFault::HostFailure {
            access: phase.access,
            direction: phase.direction,
            source,
        }))
    }

    fn physical_cause(phase: TransferPhase, vector: u8) -> CriticalFault {
        if vector == VECTOR_ADDRESS_ERROR {
            CriticalFault::AddressError {
                access: phase.access,
                direction: phase.direction,
            }
        } else {
            CriticalFault::BusAbort {
                access: phase.access,
                direction: phase.direction,
            }
        }
    }

    fn physical_unqualified<E>(
        &mut self,
        phase: TransferPhase,
        vector: u8,
        capture_missing: bool,
    ) -> TransferExit<E> {
        let cause = Self::physical_cause(phase, vector);
        self.pipeline = None;
        self.physical_state = PhysicalEntryState::FaultUnqualified { cause };
        self.physical_inspection = PhysicalInspection::Unavailable;
        TransferExit::Fault(if capture_missing {
            InstructionFault::CaptureUnavailable {
                phase: phase.phase,
                cause,
            }
        } else {
            InstructionFault::TransitionTimingUnavailable {
                phase: phase.phase,
                cause,
            }
        })
    }

    fn physical_group0<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        phase: TransferPhase,
        vector: u8,
    ) -> TransferExit<B::Error> {
        if !classifier::instruction_class(phase.ird) {
            return self.physical_unqualified(phase, vector, true);
        }
        let capture = Group0Capture {
            vector,
            internal_pc: phase.internal_pc,
            phase_sr: self.registers.status_register(),
            ird: phase.ird,
            logical_address: phase.access.logical_address,
            function_code: phase.access.function_code,
            direction: phase.direction,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        };
        self.imported_boundary = Some(ImportedGroup0Boundary {
            registers: self.registers,
            clock: self.clock,
            capture,
        });
        match self.enter_imported_group0(bus) {
            Ok(_) => TransferExit::Entered(capture),
            Err(fault) => TransferExit::Fault(InstructionFault::Entry(fault)),
        }
    }
}
