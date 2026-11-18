use super::*;
use zeff_z80::{Im1CycleKind, Im1Receipt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Im1Binding {
    pub sp: u16,
    pub halted: bool,
}

impl Plan {
    pub(super) fn prepare_im1(
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        irq: bool,
    ) -> Result<Self> {
        let request = cpu
            .next_im1_cycle(&state, !state.pending() && irq)
            .map_err(|e| anyhow::anyhow!("unsupported SOUND interrupt cycle: {e:?}"))?;
        let kind = request.kind();
        let (shape, address, value, finishes) = match kind {
            Im1CycleKind::Acknowledge { pc, .. } => (Shape::InterruptAck, pc, 0, false),
            Im1CycleKind::StackWrite {
                address,
                value,
                final_write,
            } => (Shape::Write, address, value, final_write),
        };
        let external = shape == Shape::Write && external(address);
        let plan = Self {
            shape,
            address,
            value,
            refresh: u16::from_be_bytes([cpu.regs().i, cpu.regs().r]),
            cycles: kind.t_states(),
            steps: 1,
            external,
            bank: if external { memory.bank } else { 0 },
            opaque_tail: false,
            irq,
            finishes,
            jr: None,
            copy: None,
            word: None,
            djnz: None,
            indexed: None,
            ed_word: None,
            im1: Some(Im1Binding {
                sp: cpu.regs().sp,
                halted: cpu.is_halted(),
            }),
            instruction: FetchedInstruction {
                pc: cpu.regs().pc,
                opcode: zeff_z80::Z80_INTERRUPT_ACK_OPCODE,
                cycles: kind.t_states(),
            },
        };
        plan.validate()?;
        Ok(plan)
    }

    pub(super) fn retire_im1(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        sample: Option<u8>,
        waits: u32,
    ) -> Result<(Cpu, BusState)> {
        self.retire_im1_capturing(
            cpu,
            state,
            memory,
            RetirementInput { sample, waits },
            &mut (),
        )
    }

    pub(super) fn retire_im1_capturing<C: crate::sound_trace::physical::HalfCapture>(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        input: RetirementInput,
        capture: &mut C,
    ) -> Result<(Cpu, BusState)> {
        let RetirementInput { sample, waits } = input;
        ensure!(
            sample.is_none(),
            "IM1 cycle carries an unexpected read byte"
        );
        ensure!(
            self == Self::prepare_im1(cpu, state, memory, self.irq)?,
            "IM1 cycle binding changed"
        );
        let request = cpu
            .next_im1_cycle(&state, !state.pending() && self.irq)
            .map_err(|e| anyhow::anyhow!("IM1 request: {e:?}"))?;
        let receipt = match self.shape {
            Shape::InterruptAck => Im1Receipt::Acknowledged,
            Shape::Write => Im1Receipt::Written {
                address: self.address,
                value: self.value,
            },
            _ => anyhow::bail!("invalid IM1 cycle shape"),
        };
        let completion = request
            .complete(receipt, waits)
            .map_err(|e| anyhow::anyhow!("IM1 receipt: {e:?}"))?;
        let mut next = cpu.clone();
        let mut state = state;
        let step = crate::sound_trace::physical::retirement::im1(
            &mut next, &mut state, completion, capture,
        )
        .map_err(|e| anyhow::anyhow!("IM1 retirement: {e:?}"))?;
        ensure!(
            step.complete == self.finishes
                && step.instruction.pc == self.instruction.pc
                && step.instruction.opcode == self.instruction.opcode
                && self.cycles.checked_add(waits) == Some(step.instruction.cycles),
            "IM1 completion differs"
        );
        Ok((next, state))
    }
}
