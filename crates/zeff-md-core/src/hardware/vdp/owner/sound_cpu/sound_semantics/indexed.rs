use super::*;
use zeff_z80::IndexedCbCycleKind;
use zeff_z80::{IndexedMemoryKind, IndexedMemoryReceipt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IndexedPhase {
    Displacement,
    Address,
    Read,
    Write,
}

impl Plan {
    pub(crate) fn indexed_origin_local(cpu: &Cpu) -> bool {
        cpu.last_opcode_pc()
            < if cpu.instruction_bytes().get(1) == Some(&0x36) {
                0x7efd
            } else {
                0x7efe
            }
    }
    pub(crate) fn prepare_indexed_cb(
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
    ) -> Result<Self> {
        let request = cpu
            .next_indexed_cb_cycle(&state)
            .map_err(|e| anyhow::anyhow!("indexed CB request: {e:?}"))?;
        ensure!(
            cpu.last_opcode_pc() < 0x7efd,
            "indexed CB instruction outside local profile"
        );
        let kind = request.kind();
        let (phase, shape, value) = match kind {
            IndexedCbCycleKind::Displacement { .. } => (IndexedPhase::Displacement, Shape::Read, 0),
            IndexedCbCycleKind::Terminal { .. } => (IndexedPhase::Address, Shape::Read, 0),
            IndexedCbCycleKind::Read { .. } | IndexedCbCycleKind::ReadModify { .. } => {
                (IndexedPhase::Read, Shape::Read, 0)
            }
            IndexedCbCycleKind::Write { value, .. } => (IndexedPhase::Write, Shape::Write, value),
        };
        let address = kind.address();
        let external = matches!(kind, IndexedCbCycleKind::Read { .. }) && address >= 0x8000;
        ensure!(
            address < 0x7f00 || external && memory.bank < 0x80,
            "indexed CB access outside local/banked ROM profile"
        );
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
            irq: false,
            finishes: kind.finishes(),
            im1: None,
            jr: None,
            word: None,
            djnz: None,
            copy: None,
            indexed: Some(phase),
            ed_word: None,
            instruction: request.instruction(),
        };
        plan.validate()?;
        Ok(plan)
    }

    pub(crate) fn retire_indexed_cb(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        sample: Option<u8>,
        waits: u32,
    ) -> Result<(Cpu, BusState)> {
        self.retire_indexed_cb_capturing(
            cpu,
            state,
            memory,
            RetirementInput { sample, waits },
            &mut (),
        )
    }

    pub(crate) fn retire_indexed_cb_capturing<C: crate::sound_trace::physical::HalfCapture>(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        input: RetirementInput,
        capture: &mut C,
    ) -> Result<(Cpu, BusState)> {
        let RetirementInput { sample, waits } = input;
        ensure!(
            self == Self::prepare_indexed_cb(cpu, state, memory)?,
            "indexed CB binding changed"
        );
        let request = cpu
            .next_indexed_cb_cycle(&state)
            .map_err(|e| anyhow::anyhow!("indexed CB request: {e:?}"))?;
        let receipt = if self.shape == Shape::Write {
            ensure!(sample.is_none(), "indexed CB write has a read byte");
            IndexedMemoryReceipt::Write {
                address: self.address,
                value: self.value,
            }
        } else {
            IndexedMemoryReceipt::Read {
                address: self.address,
                value: sample.ok_or_else(|| anyhow::anyhow!("missing indexed CB byte"))?,
                known_mask: 255,
            }
        };
        let completion = request
            .complete(receipt, waits)
            .map_err(|e| anyhow::anyhow!("indexed CB receipt: {e:?}"))?;
        let mut next = cpu.clone();
        let mut after = state;
        let step = crate::sound_trace::physical::retirement::indexed_cb(
            &mut next, &mut after, completion, capture,
        )
        .map_err(|e| anyhow::anyhow!("indexed CB retirement: {e:?}"))?;
        ensure!(
            step.complete == self.finishes
                && step.instruction.pc == self.instruction.pc
                && step.instruction.opcode == self.instruction.opcode
                && self.cycles.checked_add(waits) == Some(step.instruction.cycles),
            "indexed CB completion projection differs"
        );
        Ok((next, after))
    }

    pub(crate) fn prepare_indexed(
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
    ) -> Result<Self> {
        let request = cpu
            .next_indexed_memory_cycle(&state)
            .map_err(|e| anyhow::anyhow!("indexed request: {e:?}"))?;
        ensure!(
            Self::indexed_origin_local(cpu),
            "indexed instruction outside existing local read profile"
        );
        let kind = request.kind();
        if matches!(kind, IndexedMemoryKind::Immediate { .. }) {
            let base = if cpu.instruction_bytes()[0] == 0xfd {
                cpu.regs().iy
            } else {
                cpu.regs().ix
            };
            let effective = base.wrapping_add_signed(i16::from(cpu.instruction_bytes()[2] as i8));
            ensure!(
                effective < 0x7f00,
                "indexed immediate destination outside local profile"
            );
        }
        let (phase, shape, value) = match kind {
            IndexedMemoryKind::Displacement { .. } => (IndexedPhase::Displacement, Shape::Read, 0),
            IndexedMemoryKind::Address { .. } => (IndexedPhase::Address, Shape::Internal, 0),
            IndexedMemoryKind::Immediate { .. } => (IndexedPhase::Address, Shape::Read, 0),
            IndexedMemoryKind::Read { .. } | IndexedMemoryKind::ReadModify { .. } => {
                (IndexedPhase::Read, Shape::Read, 0)
            }
            IndexedMemoryKind::Write { value, .. } => (IndexedPhase::Write, Shape::Write, value),
        };
        let address = kind.address();
        let external = matches!(kind, IndexedMemoryKind::Read { .. }) && address >= 0x8000;
        let local = address < 0x7f00;
        ensure!(
            local || external && memory.bank < 0x80,
            "indexed access outside existing local/banked ROM profile"
        );
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
            irq: false,
            finishes: kind.finishes(),
            im1: None,
            jr: None,
            word: None,
            djnz: None,
            copy: None,
            indexed: Some(phase),
            ed_word: None,
            instruction: request.instruction(),
        };
        plan.validate()?;
        Ok(plan)
    }

    pub(crate) fn retire_indexed(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        sample: Option<u8>,
        waits: u32,
    ) -> Result<(Cpu, BusState)> {
        self.retire_indexed_capturing(
            cpu,
            state,
            memory,
            RetirementInput { sample, waits },
            &mut (),
        )
    }

    pub(crate) fn retire_indexed_capturing<C: crate::sound_trace::physical::HalfCapture>(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        input: RetirementInput,
        capture: &mut C,
    ) -> Result<(Cpu, BusState)> {
        let RetirementInput { sample, waits } = input;
        ensure!(
            self == Self::prepare_indexed(cpu, state, memory)?,
            "indexed cycle binding changed"
        );
        let request = cpu
            .next_indexed_memory_cycle(&state)
            .map_err(|e| anyhow::anyhow!("indexed request: {e:?}"))?;
        let receipt = match self.indexed {
            Some(IndexedPhase::Displacement | IndexedPhase::Read) => IndexedMemoryReceipt::Read {
                address: self.address,
                value: sample.ok_or_else(|| anyhow::anyhow!("missing indexed read byte"))?,
                known_mask: 255,
            },
            Some(IndexedPhase::Write) => {
                ensure!(sample.is_none(), "indexed write has read sample");
                IndexedMemoryReceipt::Write {
                    address: self.address,
                    value: self.value,
                }
            }
            Some(IndexedPhase::Address) => {
                if self.shape == Shape::Read {
                    IndexedMemoryReceipt::Read {
                        address: self.address,
                        value: sample
                            .ok_or_else(|| anyhow::anyhow!("missing indexed immediate byte"))?,
                        known_mask: 255,
                    }
                } else {
                    ensure!(
                        sample.is_none() && waits == 0,
                        "indexed internal has data/waits"
                    );
                    IndexedMemoryReceipt::Internal
                }
            }
            None => anyhow::bail!("missing indexed phase"),
        };
        let completion = request
            .complete(receipt, waits)
            .map_err(|e| anyhow::anyhow!("indexed receipt: {e:?}"))?;
        let mut next = cpu.clone();
        let mut after = state;
        let step = crate::sound_trace::physical::retirement::indexed(
            &mut next, &mut after, completion, capture,
        )
        .map_err(|e| anyhow::anyhow!("indexed retirement: {e:?}"))?;
        ensure!(
            step.complete == self.finishes
                && step.instruction.pc == self.instruction.pc
                && step.instruction.opcode == self.instruction.opcode
                && self.cycles.checked_add(waits) == Some(step.instruction.cycles),
            "indexed completion projection differs"
        );
        Ok((next, after))
    }

    pub(super) fn validate_indexed(self, phase: IndexedPhase) -> Result<()> {
        ensure!(
            !self.opaque_tail
                && !self.irq
                && self.steps == 1
                && self.instruction.cycles == self.cycles
                && self.bank < 0x80
                && (self.external || self.bank == 0)
                && match phase {
                    IndexedPhase::Displacement =>
                        self.shape == Shape::Read
                            && self.cycles == 3
                            && !self.finishes
                            && !self.external
                            && self.address < 0x7f00
                            && self.value == 0,
                    IndexedPhase::Address =>
                        matches!(self.shape, Shape::Internal | Shape::Read)
                            && self.cycles == 5
                            && !self.finishes
                            && !self.external
                            && self.address < 0x7f00
                            && self.value == 0,
                    IndexedPhase::Read =>
                        self.shape == Shape::Read
                            && matches!(self.cycles, 3 | 4)
                            && (self.finishes || self.cycles == 4)
                            && self.value == 0
                            && if self.external {
                                self.address >= 0x8000
                            } else {
                                self.address < 0x7f00
                            },
                    IndexedPhase::Write =>
                        self.shape == Shape::Write
                            && self.cycles == 3
                            && self.finishes
                            && !self.external
                            && self.address < 0x7f00,
                },
            "invalid typed indexed plan"
        );
        Ok(())
    }
}
