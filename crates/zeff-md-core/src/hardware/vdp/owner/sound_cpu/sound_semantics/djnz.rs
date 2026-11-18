use super::*;
use zeff_z80::{DjnzKind, DjnzReceipt, DjnzTail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DjnzPhase {
    M1,
    Operand,
    Internal,
}

impl Plan {
    pub(crate) fn prepare_djnz(
        cpu: &Cpu,
        state: BusState,
        irq: bool,
        tail: Option<&DjnzTail>,
    ) -> Result<Self> {
        let request = cpu
            .next_djnz_cycle(&state, tail)
            .map_err(|e| anyhow::anyhow!("DJNZ request: {e:?}"))?;
        let kind = request.kind();
        let (phase, shape, finishes) = match kind {
            DjnzKind::M1 { address, .. } => {
                ensure!(address < 0x3fff, "DJNZ outside local SRAM");
                (DjnzPhase::M1, Shape::Fetch, false)
            }
            DjnzKind::Operand { address, taken } => {
                ensure!(address < 0x4000, "DJNZ operand outside local SRAM");
                (DjnzPhase::Operand, Shape::Read, !taken)
            }
            DjnzKind::Internal { .. } => (DjnzPhase::Internal, Shape::Internal, true),
        };
        let plan = Self {
            shape,
            address: kind.address(),
            value: if phase == DjnzPhase::M1 { 0x10 } else { 0 },
            refresh: u16::from_be_bytes([cpu.regs().i, cpu.regs().r]),
            cycles: kind.t_states(),
            steps: 1,
            external: false,
            bank: 0,
            opaque_tail: false,
            irq: tail.is_none() && irq,
            finishes,
            im1: None,
            jr: None,
            copy: None,
            word: None,
            djnz: Some(phase),
            indexed: None,
            ed_word: None,
            instruction: request.instruction(),
        };
        plan.validate()?;
        Ok(plan)
    }

    pub(crate) fn retire_djnz(
        self,
        cpu: &Cpu,
        state: BusState,
        tail: Option<&DjnzTail>,
        sample: Option<u8>,
        waits: u32,
    ) -> Result<(Cpu, BusState, Option<DjnzTail>)> {
        self.retire_djnz_capturing(cpu, state, tail, RetirementInput { sample, waits }, &mut ())
    }

    pub(crate) fn retire_djnz_capturing<C: crate::sound_trace::physical::HalfCapture>(
        self,
        cpu: &Cpu,
        state: BusState,
        tail: Option<&DjnzTail>,
        input: RetirementInput,
        capture: &mut C,
    ) -> Result<(Cpu, BusState, Option<DjnzTail>)> {
        let RetirementInput { sample, waits } = input;
        ensure!(
            self == Self::prepare_djnz(cpu, state, self.irq, tail)?,
            "DJNZ binding changed"
        );
        let request = cpu
            .next_djnz_cycle(&state, tail)
            .map_err(|e| anyhow::anyhow!("DJNZ request: {e:?}"))?;
        let receipt = match self.djnz {
            Some(DjnzPhase::M1) => DjnzReceipt::M1 {
                address: self.address,
                opcode: sample.ok_or_else(|| anyhow::anyhow!("missing frozen DJNZ opcode"))?,
                known_mask: 255,
            },
            Some(DjnzPhase::Operand) => DjnzReceipt::Read {
                address: self.address,
                value: sample.ok_or_else(|| anyhow::anyhow!("missing frozen DJNZ operand"))?,
                known_mask: 255,
            },
            Some(DjnzPhase::Internal) => {
                ensure!(
                    sample.is_none() && waits == 0,
                    "DJNZ internal has data or waits"
                );
                DjnzReceipt::Internal
            }
            None => anyhow::bail!("missing DJNZ phase"),
        };
        let completion = request
            .complete(
                receipt,
                waits,
                CycleInterrupts {
                    maskable_pending: self.irq,
                    nmi_pending: false,
                },
            )
            .map_err(|e| anyhow::anyhow!("DJNZ receipt: {e:?}"))?;
        let mut next = cpu.clone();
        let mut after = state;
        let result = crate::sound_trace::physical::retirement::djnz(
            &mut next, &mut after, completion, capture,
        )
        .map_err(|e| anyhow::anyhow!("DJNZ retirement: {e:?}"))?;
        ensure!(
            result.step.complete == self.finishes
                && result.step.instruction.pc == self.instruction.pc
                && result.step.instruction.opcode == self.instruction.opcode
                && self.cycles.checked_add(waits) == Some(result.step.instruction.cycles)
                && cpu
                    .cycles()
                    .checked_add(u64::from(result.accounted_t_states))
                    == Some(next.cycles()),
            "DJNZ retirement projection differs"
        );
        Ok((next, after, result.tail))
    }

    pub(super) fn validate_djnz(self, phase: DjnzPhase) -> Result<()> {
        ensure!(
            !self.external
                && self.bank == 0
                && !self.opaque_tail
                && self.steps == 1
                && self.instruction.cycles == self.cycles
                && match phase {
                    DjnzPhase::M1 =>
                        self.shape == Shape::Fetch
                            && self.cycles == 5
                            && self.address < 0x3fff
                            && self.value == 0x10
                            && !self.finishes,
                    DjnzPhase::Operand =>
                        self.shape == Shape::Read
                            && self.cycles == 3
                            && self.address < 0x4000
                            && self.value == 0
                            && !self.irq,
                    DjnzPhase::Internal =>
                        self.shape == Shape::Internal
                            && self.cycles == 5
                            && self.address < 0x4000
                            && self.value == 0
                            && !self.irq
                            && self.finishes,
                },
            "invalid DJNZ physical plan"
        );
        Ok(())
    }
}
