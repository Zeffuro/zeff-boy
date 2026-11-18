use super::*;
use zeff_z80::{JrCycleKind, JrReceipt, JrTail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JrPhase {
    Read,
    Internal,
}

impl Plan {
    pub(crate) fn prepare_jr(
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        irq: bool,
        tail: Option<&JrTail>,
    ) -> Result<Self> {
        let request = cpu
            .next_jr_cycle(&state, tail)
            .map_err(|e| anyhow::anyhow!("JR request: {e:?}"))?;
        let kind = request.kind();
        let (shape, phase, finishes) = match kind {
            JrCycleKind::Displacement { taken, .. } => (Shape::Read, JrPhase::Read, !taken),
            JrCycleKind::Internal { .. } => (Shape::Internal, JrPhase::Internal, true),
        };
        let external = shape == Shape::Read && external(kind.address());
        let plan = Self {
            shape,
            address: kind.address(),
            value: 0,
            refresh: u16::from_be_bytes([cpu.regs().i, cpu.regs().r]),
            cycles: kind.t_states(),
            steps: 1,
            external,
            bank: if external { memory.bank } else { 0 },
            opaque_tail: false,
            irq: if tail.is_some() { false } else { irq },
            finishes,
            im1: None,
            jr: Some(phase),
            copy: None,
            word: None,
            djnz: None,
            indexed: None,
            ed_word: None,
            instruction: request.instruction(),
        };
        plan.validate()?;
        Ok(plan)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retire_jr(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        tail: Option<&JrTail>,
        sample: Option<u8>,
        waits: u32,
    ) -> Result<(Cpu, BusState, Option<JrTail>)> {
        self.retire_jr_capturing(
            RetirementState { cpu, state },
            memory,
            tail,
            RetirementInput { sample, waits },
            &mut (),
        )
    }

    pub(crate) fn retire_jr_capturing<C: crate::sound_trace::physical::HalfCapture>(
        self,
        before: RetirementState<'_>,
        memory: &SoundMemory,
        tail: Option<&JrTail>,
        input: RetirementInput,
        capture: &mut C,
    ) -> Result<(Cpu, BusState, Option<JrTail>)> {
        let RetirementState { cpu, state } = before;
        let RetirementInput { sample, waits } = input;
        ensure!(
            self == Self::prepare_jr(cpu, state, memory, self.irq, tail)?,
            "JR cycle binding changed"
        );
        let request = cpu
            .next_jr_cycle(&state, tail)
            .map_err(|e| anyhow::anyhow!("JR request: {e:?}"))?;
        let receipt = match self.jr {
            Some(JrPhase::Read) => JrReceipt::Read {
                address: self.address,
                value: sample.ok_or_else(|| anyhow::anyhow!("missing frozen JR displacement"))?,
                known_mask: 255,
            },
            Some(JrPhase::Internal) => {
                ensure!(
                    sample.is_none() && waits == 0,
                    "JR internal cycle has data or waits"
                );
                JrReceipt::Internal
            }
            None => anyhow::bail!("missing JR phase"),
        };
        let completion = request
            .complete(receipt, waits)
            .map_err(|e| anyhow::anyhow!("JR receipt: {e:?}"))?;
        let mut next = cpu.clone();
        let mut after = state;
        let result = crate::sound_trace::physical::retirement::jr(
            &mut next, &mut after, completion, capture,
        )
        .map_err(|e| anyhow::anyhow!("JR retirement: {e:?}"))?;
        ensure!(
            result.step.complete == self.finishes
                && result.step.instruction.pc == self.instruction.pc
                && result.step.instruction.opcode == self.instruction.opcode
                && self.cycles.checked_add(waits) == Some(result.step.instruction.cycles)
                && cpu
                    .cycles()
                    .checked_add(u64::from(result.accounted_t_states))
                    == Some(next.cycles()),
            "JR completion projection differs"
        );
        Ok((next, after, result.tail))
    }
}
