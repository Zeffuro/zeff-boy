use super::*;
use zeff_z80::EdWordPhase;

impl Plan {
    pub(crate) fn prepare_ed_word(cpu: &Cpu, state: BusState) -> Result<Self> {
        let request = cpu
            .next_ed_word_cycle(&state)
            .map_err(|error| anyhow::anyhow!("ED arithmetic request: {error:?}"))?;
        let kind = request.kind();
        let plan = Self {
            shape: Shape::Internal,
            address: kind.retained_address,
            value: 0,
            refresh: kind.retained_address,
            cycles: kind.t_states(),
            steps: 1,
            external: false,
            bank: 0,
            opaque_tail: false,
            irq: false,
            finishes: kind.finishes(),
            im1: None,
            jr: None,
            word: None,
            djnz: None,
            copy: None,
            indexed: None,
            ed_word: Some(kind.phase),
            instruction: request.instruction(),
        };
        plan.validate()?;
        Ok(plan)
    }

    pub(super) fn retire_ed_word(
        self,
        cpu: &Cpu,
        state: BusState,
        sample: Option<u8>,
        waits: u32,
    ) -> Result<(Cpu, BusState)> {
        self.retire_ed_word_capturing(cpu, state, sample, waits, &mut ())
    }

    pub(super) fn retire_ed_word_capturing<C: crate::sound_trace::physical::HalfCapture>(
        self,
        cpu: &Cpu,
        state: BusState,
        sample: Option<u8>,
        waits: u32,
        capture: &mut C,
    ) -> Result<(Cpu, BusState)> {
        ensure!(
            self == Self::prepare_ed_word(cpu, state)?,
            "ED arithmetic binding changed"
        );
        ensure!(sample.is_none(), "ED arithmetic internal has DATA");
        let completion = cpu
            .next_ed_word_cycle(&state)
            .and_then(|request| request.complete(waits))
            .map_err(|error| anyhow::anyhow!("ED arithmetic receipt: {error:?}"))?;
        let mut next = cpu.clone();
        let mut after = state;
        let step = crate::sound_trace::physical::retirement::ed_word(
            &mut next, &mut after, completion, capture,
        )
        .map_err(|error| anyhow::anyhow!("ED arithmetic retirement: {error:?}"))?;
        ensure!(
            step.instruction == self.instruction && step.complete == self.finishes,
            "ED arithmetic retirement differs"
        );
        Ok((next, after))
    }

    pub(super) fn validate_ed_word(self, phase: EdWordPhase) -> Result<()> {
        ensure!(
            self.shape == Shape::Internal
                && !self.external
                && self.bank == 0
                && !self.opaque_tail
                && self.steps == 1
                && !self.irq
                && self.value == 0
                && self.address == self.refresh
                && self.finishes == (phase == EdWordPhase::Internal3)
                && self.cycles
                    == if phase == EdWordPhase::Internal4 {
                        4
                    } else {
                        3
                    }
                && self.instruction.opcode == 0xed
                && self.instruction.cycles == self.cycles,
            "invalid ED arithmetic plan"
        );
        Ok(())
    }
}
