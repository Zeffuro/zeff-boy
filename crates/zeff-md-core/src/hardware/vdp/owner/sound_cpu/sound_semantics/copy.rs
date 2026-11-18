use super::*;
use zeff_z80::{CopyCycleKind, CopyReceipt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyPhase {
    Read,
    Write,
    Repeat,
}
impl Plan {
    pub(crate) fn prepare_copy(
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        irq: bool,
    ) -> Result<Self> {
        let request = cpu
            .next_copy_cycle(&state)
            .map_err(|e| anyhow::anyhow!("copy cycle: {e:?}"))?;
        let (shape, copy, value) = match request.kind() {
            CopyCycleKind::Read { .. } => (Shape::Read, CopyPhase::Read, 0),
            CopyCycleKind::Write { value, .. } => (Shape::Write, CopyPhase::Write, value),
            CopyCycleKind::Repeat { .. } => (Shape::Internal, CopyPhase::Repeat, 0),
        };
        let address = request.kind().address();
        let external = copy == CopyPhase::Read && address >= 0x8000;
        ensure!(
            address < 0x4000 || external && memory.bank < 0x80,
            "physical copy requires local SRAM or a banked ROM read"
        );
        ensure!(
            !external || cpu.regs().de() < 0x4000,
            "banked copy destination outside local SRAM"
        );
        let plan = Self {
            shape,
            address,
            value,
            refresh: u16::from_be_bytes([cpu.regs().i, cpu.regs().r]),
            cycles: request.kind().t_states(),
            steps: 1,
            external,
            bank: if external { memory.bank } else { 0 },
            opaque_tail: false,
            irq,
            finishes: request.finishes(),
            im1: None,
            jr: None,
            copy: Some(copy),
            word: None,
            djnz: None,
            indexed: None,
            ed_word: None,
            instruction: request.instruction(),
        };
        plan.validate()?;
        Ok(plan)
    }
    pub(crate) fn retire_copy(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        sample: Option<u8>,
        waits: u32,
    ) -> Result<(Cpu, BusState)> {
        self.retire_copy_capturing(
            cpu,
            state,
            memory,
            RetirementInput { sample, waits },
            &mut (),
        )
    }

    pub(crate) fn retire_copy_capturing<C: crate::sound_trace::physical::HalfCapture>(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        input: RetirementInput,
        capture: &mut C,
    ) -> Result<(Cpu, BusState)> {
        let RetirementInput { sample, waits } = input;
        ensure!(
            self == Self::prepare_copy(cpu, state, memory, self.irq)?,
            "copy cycle binding changed"
        );
        let receipt = match self.copy {
            Some(CopyPhase::Read) => CopyReceipt::Read {
                value: sample.ok_or_else(|| anyhow::anyhow!("missing copy byte"))?,
                known: 255,
            },
            Some(CopyPhase::Write) => {
                ensure!(sample.is_none(), "copy write has read sample");
                CopyReceipt::Write
            }
            Some(CopyPhase::Repeat) => {
                ensure!(sample.is_none() && waits == 0, "copy repeat has data/waits");
                CopyReceipt::Repeat
            }
            None => anyhow::bail!("missing copy phase"),
        };
        let request = cpu
            .next_copy_cycle(&state)
            .map_err(|e| anyhow::anyhow!("copy cycle: {e:?}"))?;
        let completion = request
            .complete(receipt, waits)
            .map_err(|e| anyhow::anyhow!("copy receipt: {e:?}"))?;
        let mut next = cpu.clone();
        let mut after = state;
        let step = crate::sound_trace::physical::retirement::copy(
            &mut next, &mut after, completion, capture,
        )
        .map_err(|e| anyhow::anyhow!("copy retirement: {e:?}"))?;
        ensure!(
            step.complete == self.finishes
                && self.cycles.checked_add(waits) == Some(step.instruction.cycles)
                && step.instruction.pc == self.instruction.pc
                && step.instruction.opcode == self.instruction.opcode,
            "copy semantic projection differs"
        );
        Ok((next, after))
    }
}
