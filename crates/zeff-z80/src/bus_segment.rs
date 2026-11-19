use super::*;

mod indexed_bit_cycle;
pub use indexed_bit_cycle::{IndexedCbCycleCompletion, IndexedCbCycleKind, IndexedCbCycleRequest};

mod ed_word_cycle;
pub use ed_word_cycle::{EdWordCompletion, EdWordKind, EdWordPhase, EdWordRequest};

mod djnz_cycle;
pub use djnz_cycle::{
    DjnzCompletion, DjnzKind, DjnzPhase, DjnzReceipt, DjnzRequest, DjnzRetirement, DjnzTail,
};
mod indexed_memory_cycle;
pub use indexed_memory_cycle::{
    IndexedMemoryCompletion, IndexedMemoryKind, IndexedMemoryReceipt, IndexedMemoryRequest,
};
mod copy_cycle;
pub use copy_cycle::{CopyCycleCompletion, CopyCycleKind, CopyCycleRequest, CopyReceipt};
mod im1_cycle;
mod word_add_cycle;
pub use word_add_cycle::{
    WordAddCompletion, WordAddKind, WordAddPhase, WordAddReceipt, WordAddRequest,
    WordAddRetirement, WordAddTail,
};
mod jr_cycle;
pub use jr_cycle::{
    JrCycleCompletion, JrCycleKind, JrCycleRequest, JrReceipt, JrRetirement, JrTail,
};
mod index_cycle;
pub use im1_cycle::{Im1CycleCompletion, Im1CycleKind, Im1CycleRequest, Im1Receipt};
mod m1_timing;
mod physical_cycle;
mod prefix_cycle;
mod state;
pub use physical_cycle::{
    CycleCompletion, CycleError, CycleInterrupts, CycleKind, CycleRequest, CycleSample,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BusState {
    prefix: PrefixState,
    transfer: Option<Transfer>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Transfer {
    pc: u16,
    operation: Operation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Single { opcode: u8, index: Option<u8> },
    Load(load::Load),
    Stack(stack::Stack),
    Rmw(rmw::Rmw),
    IndexedMemory(indexed_memory::IndexedMemory),
    IndexedCb(indexed_cb::IndexedCb),
    CbFetch,
    EdFetch { pc: u16 },
    EdInternal(ed::Internal),
    EdIo(ed::Io),
    EdBlock(ed::Block),
    Interrupt(interrupt::Service),
}

#[derive(Clone, Copy)]
pub(super) enum LegacyRelative {
    Jr,
    Djnz,
}

impl LegacyRelative {
    fn matches(self, opcode: u8) -> bool {
        match self {
            Self::Jr => matches!(opcode, 0x18 | 0x20 | 0x28 | 0x30 | 0x38),
            Self::Djnz => opcode == 0x10,
        }
    }
}

pub(super) fn fetch_cycles(opcode: u8) -> u32 {
    CYCLES_M1 + u32::from(opcode == 0x10)
}

impl Transfer {
    pub(super) fn interrupt(service: interrupt::Service) -> Self {
        Self {
            pc: service.pc(),
            operation: Operation::Interrupt(service),
        }
    }
}

impl BusState {
    pub fn next_readonly_bit_address(&self) -> Option<u16> {
        match self.transfer?.operation {
            Operation::Rmw(operation) => operation.readonly_bit_address(),
            _ => None,
        }
    }

    /// Classifies a running CPU; HALT and boundary interrupts override this phase.
    pub fn next_segment_reads_opcode(&self) -> bool {
        match self.transfer.map(|transfer| transfer.operation) {
            None | Some(Operation::CbFetch | Operation::EdFetch { .. }) => true,
            Some(Operation::IndexedCb(operation)) => operation.is_terminal_fetch(),
            _ => false,
        }
    }

    /// Duration of the next qualified memory phase; other phases remain unclassified.
    pub fn next_memory_cycle(&self) -> Option<u32> {
        match self.transfer?.operation {
            Operation::Single { opcode, index } => single_memory_cycle(opcode, index.is_some()),
            Operation::Load(operation) => operation.memory_cycle(),
            Operation::Stack(operation) => operation.memory_cycle(),
            Operation::IndexedMemory(operation) => operation.memory_cycle(),
            Operation::EdBlock(operation) => operation.memory_cycle(),
            _ => None,
        }
    }

    pub fn pending(&self) -> bool {
        self.prefix.pending() || self.transfer.is_some()
    }

    pub fn prefix_pending(&self) -> bool {
        self.prefix.pending()
    }

    pub fn transfer_pending(&self) -> bool {
        self.transfer.is_some()
    }
}

impl Cpu {
    /// Account waits once on the just-returned segment, before stepping or publishing it.
    pub fn extend_bus_segment(&mut self, instruction: &mut FetchedInstruction, waits: u32) -> bool {
        let Some(cycles) = instruction.cycles.checked_add(waits) else {
            return false;
        };
        let Some(total) = self.cycles.checked_add(u64::from(waits)) else {
            return false;
        };
        instruction.cycles = cycles;
        self.cycles = total;
        true
    }

    /// Splits M1 and admitted memory/address phases; other terminals stay aggregate.
    /// Pair CPU/state across clone/reset; do not mix stepping APIs or CPU-only codecs while pending.
    pub fn step_bus_segment<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        state: &mut BusState,
    ) -> Option<BoundedStep> {
        self.step_bus_segment_inner(bus, state, None)
    }

    /// Replays a saved aggregate JR without changing other admitted bus phases.
    pub fn step_bus_segment_legacy_jr<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        state: &mut BusState,
    ) -> Option<BoundedStep> {
        self.step_bus_segment_inner(bus, state, Some(LegacyRelative::Jr))
    }

    pub fn step_bus_segment_legacy_djnz<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        state: &mut BusState,
    ) -> Option<BoundedStep> {
        self.step_bus_segment_inner(bus, state, Some(LegacyRelative::Djnz))
    }

    fn step_bus_segment_inner<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        state: &mut BusState,
        legacy_relative: Option<LegacyRelative>,
    ) -> Option<BoundedStep> {
        if self.state == CpuState::Suspended {
            return None;
        }
        let Some(transfer) = state.transfer.take() else {
            return self.step_bounded_inner(
                bus,
                &mut state.prefix,
                Some(&mut state.transfer),
                legacy_relative,
            );
        };
        let (cycles, complete) = match transfer.operation {
            Operation::Interrupt(service) => {
                let (next, cycles) = service.step(self, bus);
                state.transfer = next.map(Transfer::interrupt);
                (cycles, next.is_none())
            }
            Operation::Stack(operation) => {
                let (next, cycles) = operation.step(self, bus);
                state.transfer = next.map(|operation| Transfer {
                    pc: transfer.pc,
                    operation: Operation::Stack(operation),
                });
                (cycles, next.is_none())
            }
            Operation::IndexedCb(operation) => {
                let (next, cycles) = operation.step(self, bus);
                state.transfer = next.map(|operation| Transfer {
                    pc: transfer.pc,
                    operation: Operation::IndexedCb(operation),
                });
                (cycles, next.is_none())
            }
            Operation::IndexedMemory(operation) => {
                let (next, cycles) = operation.step(self, bus);
                state.transfer = next.map(|operation| Transfer {
                    pc: transfer.pc,
                    operation: Operation::IndexedMemory(operation),
                });
                (cycles, next.is_none())
            }
            Operation::EdFetch { pc } => {
                let opcode = self.fetch_u8(bus);
                self.increment_refresh_register();
                let operation = load::Load::decode_ed(self, opcode)
                    .map(Operation::Load)
                    .or_else(|| stack::Stack::decode_ed(self, opcode).map(Operation::Stack))
                    .or_else(|| rmw::Rmw::decode_ed(self, opcode).map(Operation::Rmw))
                    .or_else(|| ed::Internal::decode(opcode).map(Operation::EdInternal))
                    .or_else(|| ed::Io::decode(self, bus, opcode).map(Operation::EdIo))
                    .or_else(|| ed::Block::decode(self, pc, opcode).map(Operation::EdBlock));
                if let Some(operation) = operation {
                    state.transfer = Some(Transfer {
                        pc: transfer.pc,
                        operation,
                    });
                    (CYCLES_M1, false)
                } else {
                    let fetches = self.last_m1_fetch_count;
                    self.last_m1_fetch_count = 1;
                    let cycles = self.execute_ed_terminal(bus, pc, opcode) - CYCLES_M1;
                    self.last_m1_fetch_count = fetches;
                    (cycles, true)
                }
            }
            Operation::EdInternal(operation) => {
                let (next, cycles) = operation.step(self, bus);
                state.transfer = next.map(|operation| Transfer {
                    pc: transfer.pc,
                    operation: Operation::EdInternal(operation),
                });
                (cycles, next.is_none())
            }
            Operation::EdIo(operation) => {
                operation.execute(self, bus, 0);
                (CYCLES_IO_MACHINE, true)
            }
            Operation::EdBlock(operation) => {
                let (next, cycles) = operation.step(self, bus, 0);
                state.transfer = next.map(|operation| Transfer {
                    pc: transfer.pc,
                    operation: Operation::EdBlock(operation),
                });
                (cycles, next.is_none())
            }
            Operation::CbFetch => {
                let opcode = self.fetch_u8(bus);
                self.increment_refresh_register();
                if opcode & 7 == REGISTER_MEMORY_INDEX {
                    state.transfer = Some(Transfer {
                        pc: transfer.pc,
                        operation: Operation::Rmw(rmw::Rmw::cb(self, opcode)),
                    });
                    (CYCLES_M1, false)
                } else {
                    self.execute_cb_terminal(bus, opcode);
                    (CYCLES_M1, true)
                }
            }
            Operation::Rmw(rmw) => {
                let (next, cycles) = rmw.step(self, bus);
                state.transfer = next.map(|rmw| Transfer {
                    pc: transfer.pc,
                    operation: Operation::Rmw(rmw),
                });
                (cycles, next.is_none())
            }
            Operation::Load(load) => {
                let next = load.step(self, bus);
                state.transfer = next.map(|load| Transfer {
                    pc: transfer.pc,
                    operation: Operation::Load(load),
                });
                (CYCLES_MEMORY_TRANSFER, next.is_none())
            }
            Operation::Single { opcode, index } => (
                match index {
                    Some(index) => {
                        self.execute_index_terminal(bus, transfer.pc, index, opcode)
                            - Z80_PREFIX_OVERHEAD
                            - fetch_cycles(opcode)
                    }
                    None => {
                        self.execute_unprefixed(bus, transfer.pc, opcode) - fetch_cycles(opcode)
                    }
                },
                true,
            ),
        };
        if complete {
            state.prefix = PrefixState::default();
            self.finish_instruction(cycles);
        } else {
            self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        }
        Some(BoundedStep {
            instruction: FetchedInstruction {
                pc: transfer.pc,
                opcode: self.last_opcode,
                cycles,
            },
            complete,
        })
    }

    pub(super) fn defer_transfer(
        &mut self,
        slot: Option<&mut Option<Transfer>>,
        pc: u16,
        opcode: u8,
        index: Option<u8>,
        legacy_relative: Option<LegacyRelative>,
    ) -> bool {
        let Some(slot) = slot else {
            return false;
        };
        let operation = if let Some(load) = load::Load::decode(self, opcode, index) {
            Operation::Load(load)
        } else if let Some(operation) = stack::Stack::decode(self, opcode, index) {
            Operation::Stack(operation)
        } else if let Some(operation) =
            index.and_then(|prefix| indexed_memory::IndexedMemory::decode(self, opcode, prefix))
        {
            Operation::IndexedMemory(operation)
        } else if opcode == Z80_PREFIX_ED {
            Operation::EdFetch {
                pc: self.regs.pc.wrapping_sub(1),
            }
        } else if opcode == Z80_PREFIX_CB {
            match index {
                Some(prefix) => Operation::IndexedCb(indexed_cb::IndexedCb::new(self, prefix)),
                None => Operation::CbFetch,
            }
        } else if let Some(rmw) = index
            .is_none()
            .then(|| rmw::Rmw::inc_dec(self, opcode))
            .flatten()
        {
            Operation::Rmw(rmw)
        } else if single_transfer(opcode, index.is_some())
            && legacy_relative.is_none_or(|legacy| !legacy.matches(opcode))
        {
            Operation::Single { opcode, index }
        } else {
            return false;
        };
        *slot = Some(Transfer { pc, operation });
        self.cycles = self.cycles.wrapping_add(u64::from(fetch_cycles(opcode)));
        true
    }
}

fn single_memory_cycle(opcode: u8, indexed: bool) -> Option<u32> {
    let memory = match opcode {
        0x02 | 0x0a | 0x12 | 0x1a | 0x06 | 0x0e | 0x16 | 0x1e | 0x26 | 0x2e | 0x3e | 0xc6
        | 0xce | 0xd6 | 0xde | 0xe6 | 0xee | 0xf6 | 0xfe => true,
        0x46
        | 0x4e
        | 0x56
        | 0x5e
        | 0x66
        | 0x6e
        | 0x7e
        | 0x70..=0x75
        | 0x77
        | 0x86
        | 0x8e
        | 0x96
        | 0x9e
        | 0xa6
        | 0xae
        | 0xb6
        | 0xbe => !indexed,
        _ => false,
    };
    memory.then_some(CYCLES_MEMORY_TRANSFER)
}

fn single_transfer(opcode: u8, indexed: bool) -> bool {
    match opcode {
        0x02 | 0x0a | 0x12 | 0x1a | 0x06 | 0x0e | 0x16 | 0x1e | 0x26 | 0x2e | 0x3e | 0xc6
        | 0xce | 0xd6 | 0xde | 0xe6 | 0xee | 0xf6 | 0xfe | 0x10 | 0x18 | 0x20 | 0x28 | 0x30
        | 0x38 => true,
        0x46
        | 0x4e
        | 0x56
        | 0x5e
        | 0x66
        | 0x6e
        | 0x7e
        | 0x70..=0x75
        | 0x77
        | 0x86
        | 0x8e
        | 0x96
        | 0x9e
        | 0xa6
        | 0xae
        | 0xb6
        | 0xbe => !indexed,
        _ => false,
    }
}

#[cfg(test)]
#[path = "bus_segment/wait_tests.rs"]
mod wait_tests;

#[cfg(test)]
#[path = "bus_segment/operand_tests.rs"]
mod operand_tests;

#[cfg(test)]
#[path = "bus_segment/prefix_tests.rs"]
mod prefix_tests;

#[cfg(test)]
#[path = "bus_segment/rmw_tests.rs"]
mod rmw_tests;

#[cfg(test)]
#[path = "bus_segment/index_tests.rs"]
mod index_tests;

#[cfg(test)]
#[path = "bus_segment/m1_tests.rs"]
mod m1_tests;

#[cfg(test)]
#[path = "bus_segment/call_tests.rs"]
mod call_tests;

#[cfg(test)]
#[path = "bus_segment/retirement_evidence_tests.rs"]
mod retirement_evidence_tests;
