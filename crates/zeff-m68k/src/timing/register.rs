use super::*;
use crate::bus::M68kBus;
use crate::constants::TRANSFER_CLOCKS;
use crate::ea::Size;

#[derive(Clone, Copy)]
enum FetchOrder {
    Early,
    Late,
}

fn route(op: u16) -> Option<FetchOrder> {
    let mode = (op >> 3) & 7;
    let size = (op >> 6) & 3;
    if matches!(op & 0xf1f8, 0x80c0 | 0x81c0) {
        return Some(FetchOrder::Late);
    }
    if matches!(
        op & 0xf1f8,
        0xc0c0 | 0xc1c0 | 0xc100 | 0x8100 | 0xc140 | 0xc148 | 0xc188
    ) || matches!(op & 0xfff8, 0x4840 | 0x4880 | 0x48c0 | 0x4800)
        || op & 0xf000 == 0xe000 && size < 3
        || op & 0xf100 == 0x0100 && mode == 0
        || matches!(op & 0xf138, 0xd100 | 0x9100) && size < 3
        || op & 0xf000 == 0x5000 && size < 3 && (mode == 0 || mode == 1 && size != 0)
        || mode == 0
            && size < 3
            && matches!(op & 0xff00, 0x4000 | 0x4200 | 0x4400 | 0x4600 | 0x4a00)
    {
        return Some(FetchOrder::Early);
    }
    let opmode = (op >> 6) & 7;
    let admitted = match op >> 12 {
        8 | 12 => mode == 0 && opmode <= 2,
        9 | 13 => {
            (opmode <= 2 && (mode == 0 || mode == 1 && opmode != 0))
                || matches!(opmode, 3 | 7) && mode <= 1
        }
        11 => {
            (opmode <= 2 && (mode == 0 || mode == 1 && opmode != 0))
                || matches!(opmode, 3 | 7) && mode <= 1
                || (4..=6).contains(&opmode) && mode == 0
        }
        _ => false,
    };
    admitted.then_some(FetchOrder::Early)
}

#[derive(Default)]
struct DenyBus {
    attempted: bool,
}
impl M68kBus for DenyBus {
    type Error = ();
    fn read(&mut self, _: BusAccess) -> Result<u16, ()> {
        self.attempted = true;
        Err(())
    }
    fn write(&mut self, _: BusAccess, _: u16) -> Result<(), ()> {
        self.attempted = true;
        Err(())
    }
}

impl PrefetchCpu {
    pub(super) fn register_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let Some(order) = route(pipeline.words[0]) else {
            return Ok(false);
        };
        let pc = pipeline.address;
        let opcode = pipeline.words[0];
        let unsupported = || TimingFault::UnsupportedInstruction { pc, opcode };
        let mut candidate = self.registers;
        let mut probe = DenyBus::default();
        let execution = crate::instruction::execute(&mut candidate, &mut probe, pc, opcode)
            .map_err(|_| unsupported())?;
        if probe.attempted
            || execution.exception.is_some()
            || execution.stopped
            || candidate.pc != pc.wrapping_add(2)
        {
            return Err(unsupported());
        }
        let internal = execution
            .nominal_cycles
            .checked_sub(TRANSFER_CLOCKS)
            .ok_or_else(unsupported)?;
        if matches!(order, FetchOrder::Late) && internal != 0 {
            self.idle(bus, internal)?;
        }
        let staged_unary = matches!(opcode & 0xff00, 0x4000 | 0x4200 | 0x4400 | 0x4600);
        let register = usize::from(opcode & 7);
        if opcode & 0xff00 == 0x4a00 || staged_unary {
            self.reserve_transfer()?;
            self.registers
                .set_status_register(candidate.status_register());
        }
        if staged_unary {
            let mask = if opcode & 0x00c0 == 0 {
                Size::Byte.mask()
            } else {
                Size::Word.mask()
            };
            self.registers.d[register] =
                self.registers.d[register] & !mask | candidate.d[register] & mask;
        }
        let next = self.fetch(bus, pc.wrapping_add(4))?;
        if staged_unary && opcode & 0x00c0 == 0x0080 {
            self.registers.d[register] = candidate.d[register];
        }
        if matches!(order, FetchOrder::Early) && internal != 0 {
            self.idle(bus, internal)?;
        }
        self.registers = candidate;
        self.pipeline = Some(Pipeline {
            address: candidate.pc,
            words: [pipeline.words[1], next],
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
