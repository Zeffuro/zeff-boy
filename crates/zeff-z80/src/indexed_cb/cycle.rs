use super::*;

impl IndexedCb {
    pub(crate) fn typed_kind(
        self,
        cpu: &Cpu,
        prefix: u8,
    ) -> Result<IndexedCbCycleKind, CycleError> {
        let base = if prefix == Z80_PREFIX_FD {
            cpu.regs.iy
        } else {
            cpu.regs.ix
        };
        let effective = base.wrapping_add_signed(i16::from(cpu.instruction_bytes[2] as i8));
        match self.0 {
            Phase::Displacement(captured)
                if cpu.instruction_byte_count == 2 && captured == base =>
            {
                let previous_r = (cpu.regs.r & 0x80) | (cpu.regs.r.wrapping_sub(1) & 0x7f);
                Ok(IndexedCbCycleKind::Displacement {
                    address: cpu.regs.pc,
                    preceding_refresh: u16::from_be_bytes([cpu.regs.i, previous_r]),
                })
            }
            Phase::Terminal(address) if cpu.instruction_byte_count == 3 && address == effective => {
                Ok(IndexedCbCycleKind::Terminal {
                    address: cpu.regs.pc,
                })
            }
            Phase::Memory(operation)
                if cpu.instruction_byte_count == 4
                    && (cpu.memptr_profile == MemptrProfile::Legacy
                        || cpu.memptr == Some(effective)) =>
            {
                operation
                    .indexed_cb_cycle(cpu, effective, cpu.instruction_bytes[3])
                    .ok_or(CycleError::UnsupportedCycle)
            }
            _ => Err(CycleError::UnsupportedCycle),
        }
    }
}
