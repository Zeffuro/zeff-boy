use super::*;
use zeff_z80::{WordAddKind, WordAddPhase, WordAddReceipt, WordAddTail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WordPhase {
    M1,
    Internal4,
    Internal3,
}

impl WordPhase {
    pub fn internal(self) -> bool {
        self != Self::M1
    }
}

impl Plan {
    pub(crate) fn prepare_word(
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        irq: bool,
        tail: Option<&WordAddTail>,
    ) -> Result<Self> {
        let request = cpu
            .next_word_add_cycle(&state, tail)
            .map_err(|e| anyhow::anyhow!("ADD16 request: {e:?}"))?;
        let kind = request.kind();
        let (phase, shape, opcode) = match kind {
            WordAddKind::M1 { address, .. } => {
                ensure!(address < 0x4000, "ADD16 M1 outside local SRAM");
                (
                    WordPhase::M1,
                    Shape::Fetch,
                    memory
                        .read(address)
                        .ok_or_else(|| anyhow::anyhow!("missing ADD16 opcode"))?,
                )
            }
            WordAddKind::Internal { phase, .. } => (
                match phase {
                    WordAddPhase::Internal4 => WordPhase::Internal4,
                    WordAddPhase::Internal3 => WordPhase::Internal3,
                },
                Shape::Internal,
                tail.ok_or_else(|| anyhow::anyhow!("missing ADD16 tail"))?
                    .opcode(),
            ),
        };
        let plan = Self {
            shape,
            address: kind.address(),
            value: if phase == WordPhase::M1 { opcode } else { 0 },
            refresh: u16::from_be_bytes([cpu.regs().i, cpu.regs().r]),
            cycles: kind.t_states(),
            steps: 1,
            external: false,
            bank: 0,
            opaque_tail: false,
            irq: if tail.is_some() { false } else { irq },
            finishes: phase == WordPhase::Internal3,
            im1: None,
            jr: None,
            copy: None,
            word: Some(phase),
            djnz: None,
            indexed: None,
            ed_word: None,
            instruction: request
                .instruction(opcode)
                .map_err(|e| anyhow::anyhow!("ADD16 instruction: {e:?}"))?,
        };
        plan.validate()?;
        Ok(plan)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retire_word(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        tail: Option<&WordAddTail>,
        sample: Option<u8>,
        waits: u32,
    ) -> Result<(Cpu, BusState, Option<WordAddTail>)> {
        self.retire_word_capturing(
            RetirementState { cpu, state },
            memory,
            tail,
            RetirementInput { sample, waits },
            &mut (),
        )
    }

    pub(crate) fn retire_word_capturing<C: crate::sound_trace::physical::HalfCapture>(
        self,
        before: RetirementState<'_>,
        memory: &SoundMemory,
        tail: Option<&WordAddTail>,
        input: RetirementInput,
        capture: &mut C,
    ) -> Result<(Cpu, BusState, Option<WordAddTail>)> {
        let RetirementState { cpu, state } = before;
        let RetirementInput { sample, waits } = input;
        ensure!(
            self == Self::prepare_word(cpu, state, memory, self.irq, tail)?,
            "ADD16 cycle binding changed"
        );
        let request = cpu
            .next_word_add_cycle(&state, tail)
            .map_err(|e| anyhow::anyhow!("ADD16 request: {e:?}"))?;
        let receipt = if self.word == Some(WordPhase::M1) {
            let opcode = sample.ok_or_else(|| anyhow::anyhow!("missing frozen ADD16 opcode"))?;
            ensure!(
                opcode == self.value,
                "ADD16 opcode changed before M1 retirement"
            );
            WordAddReceipt::M1 {
                address: self.address,
                opcode,
                known_mask: 255,
            }
        } else {
            ensure!(
                sample.is_none() && waits == 0,
                "ADD16 internal has data or waits"
            );
            WordAddReceipt::Internal
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
            .map_err(|e| anyhow::anyhow!("ADD16 receipt: {e:?}"))?;
        let mut next = cpu.clone();
        let mut after = state;
        let result = crate::sound_trace::physical::retirement::word(
            &mut next, &mut after, completion, capture,
        )
        .map_err(|e| anyhow::anyhow!("ADD16 retirement: {e:?}"))?;
        ensure!(
            result.step.complete == self.finishes
                && result.step.instruction.pc == self.instruction.pc
                && result.step.instruction.opcode == self.instruction.opcode
                && self.cycles.checked_add(waits) == Some(result.step.instruction.cycles)
                && cpu
                    .cycles()
                    .checked_add(u64::from(result.accounted_t_states))
                    == Some(next.cycles()),
            "ADD16 retirement projection differs"
        );
        Ok((next, after, result.tail))
    }

    pub(super) fn validate_word(self, phase: WordPhase) -> Result<()> {
        ensure!(
            !self.external
                && self.bank == 0
                && !self.opaque_tail
                && self.steps == 1
                && self.finishes == (phase == WordPhase::Internal3)
                && self.instruction.cycles == self.cycles
                && match phase {
                    WordPhase::M1 =>
                        self.shape == Shape::Fetch
                            && self.cycles == 4
                            && self.address < 0x4000
                            && matches!(self.value, 0x09 | 0x19 | 0x29 | 0x39),
                    WordPhase::Internal4 | WordPhase::Internal3 =>
                        self.shape == Shape::Internal
                            && self.cycles == (if phase == WordPhase::Internal4 { 4 } else { 3 })
                            && self.value == 0
                            && !self.irq
                            && self.address == self.refresh,
                },
            "invalid ADD16 physical plan"
        );
        Ok(())
    }
}
