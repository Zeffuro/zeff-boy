use super::*;
pub(crate) use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

const VERSION: u16 = 1;
const MAX_BYTES: usize = 256;

impl Cpu {
    /// Save the CPU together with its pending bus-segment continuation.
    pub fn write_continuation(&self, bus: &BusState, w: &mut Writer) {
        w.write_u16(VERSION);
        w.write_section(|w| {
            w.write_u16(self.reset_state.pc);
            w.write_u16(self.reset_state.sp);
            self.write_state(w);
            w.write_bool(self.last_step_was_interrupt);
            w.write_bytes(&self.instruction_bytes);
            w.write_u8(self.instruction_byte_count);
            w.write_u8(self.last_m1_fetch_count);
            bus.write_continuation(w);
        });
    }

    /// Restore both members only after the complete bounded payload validates.
    pub fn read_continuation(
        &mut self,
        bus: &mut BusState,
        r: &mut Reader<'_>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            r.read_u16()? == VERSION,
            "unsupported Z80 continuation version"
        );
        let mut r = Reader::new(r.read_slice(MAX_BYTES)?);
        let mut cpu = self.clone();
        cpu.reset_state = ResetState::new(r.read_u16()?, r.read_u16()?);
        cpu.regs = read_registers(&mut r)?;
        cpu.shadow = read_shadow_registers(&mut r)?;
        cpu.state = byte_to_cpu_state(r.read_u8()?)?;
        cpu.interrupt_mode = byte_to_interrupt_mode(r.read_u8()?)?;
        cpu.interrupt_flip_flop_1 = r.read_bool()?;
        cpu.interrupt_flip_flop_2 = r.read_bool()?;
        cpu.enable_interrupts_delay = r.read_u8()?;
        cpu.cycles = r.read_u64()?;
        cpu.last_opcode_pc = r.read_u16()?;
        cpu.last_opcode = r.read_u8()?;
        cpu.trap = read_trap(&mut r)?;
        cpu.last_step_was_interrupt = r.read_bool()?;
        r.read_exact(&mut cpu.instruction_bytes)?;
        cpu.instruction_byte_count = bounded_byte(&mut r, 4)?;
        cpu.last_m1_fetch_count = r.read_u8()?;
        let candidate_bus = BusState::read_continuation(&mut r)?;
        anyhow::ensure!(r.is_exhausted(), "trailing Z80 continuation data");
        *self = cpu;
        *bus = candidate_bus;
        Ok(())
    }
}

pub(crate) fn bounded_byte(r: &mut Reader<'_>, max: u8) -> anyhow::Result<u8> {
    let value = r.read_u8()?;
    anyhow::ensure!(value <= max, "invalid Z80 continuation index: {value}");
    Ok(value)
}

pub(crate) fn write_option_u8(value: Option<u8>, w: &mut Writer) {
    w.write_bool(value.is_some());
    if let Some(value) = value {
        w.write_u8(value);
    }
}

pub(crate) fn read_option_u8(r: &mut Reader<'_>) -> anyhow::Result<Option<u8>> {
    Ok(if r.read_bool()? {
        Some(r.read_u8()?)
    } else {
        None
    })
}

pub(crate) fn write_option_u16(value: Option<u16>, w: &mut Writer) {
    w.write_bool(value.is_some());
    if let Some(value) = value {
        w.write_u16(value);
    }
}

pub(crate) fn read_option_u16(r: &mut Reader<'_>) -> anyhow::Result<Option<u16>> {
    Ok(if r.read_bool()? {
        Some(r.read_u16()?)
    } else {
        None
    })
}

pub(crate) fn read_index(r: &mut Reader<'_>) -> anyhow::Result<u8> {
    let value = r.read_u8()?;
    anyhow::ensure!(
        matches!(value, Z80_PREFIX_DD | Z80_PREFIX_FD),
        "invalid Z80 index prefix"
    );
    Ok(value)
}

#[cfg(test)]
mod tests;
