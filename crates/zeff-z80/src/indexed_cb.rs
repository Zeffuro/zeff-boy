use super::*;

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct IndexedCb(Phase);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Displacement(u16),
    Terminal(u16),
    Memory(rmw::Rmw),
}

impl IndexedCb {
    pub(super) fn new(cpu: &Cpu, prefix: u8) -> Self {
        Self(Phase::Displacement(if prefix == Z80_PREFIX_FD {
            cpu.regs.iy
        } else {
            cpu.regs.ix
        }))
    }

    pub(super) fn step<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> (Option<Self>, u32) {
        let (next, cycles) = match self.0 {
            Phase::Displacement(base) => {
                let displacement = cpu.fetch_u8(bus) as i8;
                let address = base.wrapping_add_signed(i16::from(displacement));
                (Phase::Terminal(address), CYCLES_MEMORY_TRANSFER)
            }
            Phase::Terminal(address) => {
                // The terminal byte is a memory read, with two internal clocks and no refresh.
                let opcode = cpu.fetch_u8(bus);
                (
                    Phase::Memory(rmw::Rmw::indexed_cb(address, opcode)),
                    CYCLES_INDEX_CB_TERMINAL,
                )
            }
            Phase::Memory(operation) => {
                let (next, cycles) = operation.step(cpu, bus);
                return (next.map(|operation| Self(Phase::Memory(operation))), cycles);
            }
        };
        (Some(Self(next)), cycles)
    }

    pub(super) fn execute<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> u32 {
        let mut pending = Some(self);
        let mut cycles = Z80_PREFIX_OVERHEAD + CYCLES_M1;
        while let Some(operation) = pending {
            let (next, elapsed) = operation.step(cpu, bus);
            pending = next;
            cycles += elapsed;
        }
        cycles
    }
}
