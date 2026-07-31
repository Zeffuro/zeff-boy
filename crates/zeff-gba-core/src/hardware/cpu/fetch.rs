use super::super::{bus::Bus, timing};
use super::decode::decode_stub;
use super::*;

impl Cpu {
    pub(crate) fn peek_decode_stub(&self, bus: &Bus) -> FetchedInstruction {
        let instruction_set = self.instruction_set();
        let width_bytes = instruction_set.width_bytes();
        let pc = align_pc(self.pc(), instruction_set);
        if let Some(fetched) = self
            .prefetch_queue
            .front()
            .copied()
            .filter(|fetched| fetched.pc == pc && fetched.instruction_set == instruction_set)
        {
            return fetched;
        }

        fetch_instruction_at(
            bus,
            pc,
            instruction_set,
            width_bytes,
            self.next_fetch_sequential,
        )
    }

    pub(crate) fn fetch_decode_stub(&mut self, bus: &Bus) -> FetchedInstruction {
        let instruction_set = self.instruction_set();
        let width_bytes = instruction_set.width_bytes();
        let pc = align_pc(self.pc(), instruction_set);
        self.last_opcode_pc = pc;

        let fetched = self.fetch_from_prefetch_queue(bus, pc, instruction_set, width_bytes);

        if self.state == CpuState::Running {
            self.regs[15] = pc.wrapping_add(u32::from(width_bytes));
            if self.swi_wait_return_pc == Some(pc) {
                self.bios_protected_read_latch = POST_SWI_BIOS_READ_LATCH;
                self.swi_wait_return_pc = None;
            }
            self.track_bios_fetch(fetched);
            self.fill_prefetch_queue(bus, instruction_set, width_bytes);
            self.cycles = self.cycles.wrapping_add(u64::from(fetched.fetch_cycles));
            self.next_fetch_sequential = true;
            self.last_fetch = Some(fetched);
        }

        fetched
    }

    fn fetch_from_prefetch_queue(
        &mut self,
        bus: &Bus,
        pc: u32,
        instruction_set: InstructionSet,
        width_bytes: u8,
    ) -> FetchedInstruction {
        let queued_front_matches = matches!(
            self.prefetch_queue.front(),
            Some(fetched) if fetched.pc == pc && fetched.instruction_set == instruction_set
        );
        if queued_front_matches {
            return self
                .prefetch_queue
                .remove(0)
                .expect("prefetch queue front existed");
        }

        self.prefetch_queue.clear();
        fetch_instruction_at(
            bus,
            pc,
            instruction_set,
            width_bytes,
            self.next_fetch_sequential,
        )
    }

    fn fill_prefetch_queue(&mut self, bus: &Bus, instruction_set: InstructionSet, width_bytes: u8) {
        let width = u32::from(width_bytes);
        let mut pc = self.prefetch_queue.back().map_or_else(
            || align_pc(self.pc(), instruction_set),
            |fetched| fetched.pc.wrapping_add(width),
        );
        while self.prefetch_queue.len() < PREFETCH_QUEUE_LEN {
            let aligned_pc = align_pc(pc, instruction_set);
            let fetched = fetch_instruction_at(bus, aligned_pc, instruction_set, width_bytes, true);
            self.track_bios_fetch(fetched);
            self.prefetch_queue.push_back(fetched);
            pc = aligned_pc.wrapping_add(width);
        }
    }

    pub(super) fn flush_prefetch_queue(&mut self) {
        self.prefetch_queue.clear();
    }
}

fn align_pc(pc: u32, instruction_set: InstructionSet) -> u32 {
    match instruction_set {
        InstructionSet::Arm => pc & !3,
        InstructionSet::Thumb => pc & !1,
    }
}

fn fetch_instruction_at(
    bus: &Bus,
    pc: u32,
    instruction_set: InstructionSet,
    width_bytes: u8,
    sequential: bool,
) -> FetchedInstruction {
    let raw = match instruction_set {
        InstructionSet::Arm => bus.read32(pc),
        InstructionSet::Thumb => u32::from(bus.read16(pc)),
    };
    let fetch_cycles = timing::instruction_fetch_cycles(pc, width_bytes, sequential);

    FetchedInstruction {
        pc,
        raw,
        instruction_set,
        width_bytes,
        fetch_cycles,
        decoded: decode_stub(raw, instruction_set),
    }
}
