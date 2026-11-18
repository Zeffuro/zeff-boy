use super::*;

impl Plan {
    pub fn reset_compatible(self, cpu: &Cpu, state: BusState, memory: &SoundMemory) -> Result<()> {
        self.reset_compatible_inner(cpu, state, memory, false)
    }
    pub(crate) fn reset_after_copy(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
    ) -> Result<()> {
        self.reset_compatible_inner(cpu, state, memory, true)
    }
    fn reset_compatible_inner(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        prefix: bool,
    ) -> Result<()> {
        let pending_prefix = prefix && matches!(self.value, 0xcb | 0xed | 0xdd | 0xfd);
        ensure!(
            self.shape == Shape::Fetch
                && !self.external
                && !self.opaque_tail
                && self.address < 0x4000
                && self.cycles == 4
                && self.steps == 1
                && (self.finishes || pending_prefix)
                && !state.pending()
                && !cpu.is_halted()
                && !cpu.interrupt_enable_pending(),
            "RESET entry is not an ordinary local four-clock M1"
        );
        let (mut actual, after, instruction, accesses) = run(
            cpu,
            state,
            memory,
            None,
            0,
            self.irq,
            Some((self.address, self.value)),
            1,
        )?;
        ensure!(
            instruction == self.instruction
                && (!after.pending() || pending_prefix)
                && accesses.len() == 1
                && !accesses[0].write
                && actual.state() == cpu.state()
                && actual.interrupt_mode() == cpu.interrupt_mode()
                && actual.interrupts_enabled() == cpu.interrupts_enabled()
                && actual.saved_interrupts_enabled() == cpu.saved_interrupts_enabled()
                && !actual.interrupt_enable_pending(),
            "RESET M1 changes execution or interrupt state"
        );
        let (mut reference, mut reference_state, _, _) = run(
            cpu,
            state,
            memory,
            None,
            0,
            self.irq,
            Some((self.address, 0)),
            1,
        )?;
        let mut after = after;
        actual
            .apply_qualified_reset(&mut after)
            .map_err(|e| anyhow::anyhow!("RESET semantic projection: {e:?}"))?;
        reference
            .apply_qualified_reset(&mut reference_state)
            .map_err(|e| anyhow::anyhow!("RESET reference projection: {e:?}"))?;
        ensure!(
            actual == reference && after == reference_state,
            "RESET M1 has effects beyond reset-defined state"
        );
        Ok(())
    }
}
