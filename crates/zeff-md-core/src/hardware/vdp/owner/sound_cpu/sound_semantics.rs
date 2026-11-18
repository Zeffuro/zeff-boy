mod reader;
mod retirement;
use reader::run;
mod copy;
mod ed_word;
pub(super) use zeff_z80::EdWordPhase;
mod reset;
pub(super) use copy::CopyPhase;
mod djnz;
mod im1;
mod indexed;
mod jr;
mod word;
pub(super) use djnz::DjnzPhase;
pub(super) use indexed::IndexedPhase;
pub(super) use jr::JrPhase;
pub(super) use word::WordPhase;
mod state;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use crate::hardware::{audio::Audio, sound::SoundMemory};
use anyhow::{Result, ensure};
pub(super) use im1::Im1Binding;
use zeff_z80::{
    BusState, Cpu, CycleInterrupts, CycleKind, CycleSample, FetchedInstruction, Z80Bus,
};

pub(super) struct RetirementState<'a> {
    pub cpu: &'a Cpu,
    pub state: BusState,
}
pub(super) struct RetirementMemory<'a> {
    pub memory: &'a SoundMemory,
    pub audio: &'a Audio,
    pub tick: u64,
}

#[derive(Clone, Copy)]
pub(super) struct RetirementInput {
    pub sample: Option<u8>,
    pub waits: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Fetch,
    Read,
    Write,
    Internal,
    InterruptAck,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Plan {
    pub shape: Shape,
    pub address: u16,
    pub value: u8,
    pub refresh: u16,
    pub cycles: u32,
    pub steps: u8,
    pub external: bool,
    pub bank: u16,
    pub opaque_tail: bool,
    pub irq: bool,
    pub finishes: bool,
    pub im1: Option<Im1Binding>,
    pub jr: Option<JrPhase>,
    pub word: Option<WordPhase>,
    pub djnz: Option<DjnzPhase>,
    pub copy: Option<CopyPhase>,
    pub indexed: Option<IndexedPhase>,
    pub ed_word: Option<EdWordPhase>,
    pub instruction: FetchedInstruction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Access {
    address: u16,
    value: u8,
    write: bool,
}

pub(super) fn external(address: u16) -> bool {
    address >= 0x8000 || address & 0xff00 == 0x7f00
}

pub(super) fn local_byte(
    memory: &SoundMemory,
    audio: &Audio,
    tick: u64,
    address: u16,
) -> Result<u8> {
    if let Some(value) = memory.read(address) {
        return Ok(value);
    }
    if (0x4000..=0x5fff).contains(&address) {
        return Ok(if address & 3 == 0 {
            audio.opn2_status_at(tick)
        } else {
            0
        });
    }
    anyhow::bail!("unowned local SOUND read {address:04x}")
}

impl Plan {
    pub fn memory_predecessor_shape(self) -> bool {
        matches!(self.shape, Shape::Read | Shape::Write)
            && self.finishes
            && self.cycles == 3
            && self.steps == 1
            && !self.external
            && !self.opaque_tail
            && self.address < 0x4000
    }
    pub fn reset_predecessor(self, cpu: &Cpu, state: BusState) -> bool {
        if !self.memory_predecessor_shape()
            || cpu.interrupts_enabled()
            || cpu.interrupt_enable_pending()
        {
            return false;
        }
        let Ok(request) = cpu.next_physical_cycle(&state) else {
            return false;
        };
        match request.kind() {
            CycleKind::MemoryRead { address } => {
                self.shape == Shape::Read && address == self.address
            }
            CycleKind::MemoryWrite { address, value } => {
                self.shape == Shape::Write && address == self.address && value == self.value
            }
            _ => false,
        }
    }

    pub fn prepare(
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        audio: &Audio,
        tick: u64,
        irq: bool,
    ) -> Result<Self> {
        Self::prepare_inner(cpu, state, memory, Some(audio), tick, irq, None)
    }
    #[allow(clippy::too_many_arguments)]
    fn prepare_inner(
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        audio: Option<&Audio>,
        tick: u64,
        irq: bool,
        forced: Option<(u16, u8)>,
    ) -> Result<Self> {
        ensure!(
            !cpu.is_suspended() && cpu.trap().is_none(),
            "invalid SOUND semantic boundary"
        );
        if state.interrupt_pending()
            || !state.pending()
                && irq
                && cpu.interrupts_enabled()
                && !cpu.interrupt_enable_pending()
        {
            return Self::prepare_im1(cpu, state, memory, irq);
        }
        if cpu.next_ed_word_cycle(&state).is_ok() {
            return Self::prepare_ed_word(cpu, state);
        }
        if cpu.next_indexed_memory_cycle(&state).is_ok() {
            return Self::prepare_indexed(cpu, state, memory);
        }
        if cpu.indexed_cb_cycle_pending(&state) {
            return Self::prepare_indexed_cb(cpu, state, memory);
        }
        if memory.read(cpu.regs().pc) == Some(0x10)
            && !(irq
                && cpu.interrupts_enabled()
                && !cpu.interrupt_enable_pending()
                && !state.pending())
            && let Ok(plan) = Self::prepare_djnz(cpu, state, irq, None)
        {
            return Ok(plan);
        }
        if cpu.next_copy_cycle(&state).is_ok() {
            return Self::prepare_copy(cpu, state, memory, irq);
        }
        if cpu.next_jr_cycle(&state, None).is_ok() {
            return Self::prepare_jr(cpu, state, memory, irq, None);
        }
        if memory
            .read(cpu.regs().pc)
            .is_some_and(|opcode| matches!(opcode, 0x09 | 0x19 | 0x29 | 0x39))
            && !(irq
                && cpu.interrupts_enabled()
                && !cpu.interrupt_enable_pending()
                && !state.pending())
            && let Ok(plan) = Self::prepare_word(cpu, state, memory, irq, None)
        {
            return Ok(plan);
        }
        let (_, mut after_state, mut instruction, mut accesses) =
            run(cpu, state, memory, audio, tick, irq, forced, 1)?;
        let mut steps = 1;
        let fetch = state.next_segment_reads_opcode() || cpu.is_halted();
        let first = accesses.first().copied();
        let shape = match first {
            Some(a) if a.write => Shape::Write,
            Some(_) => {
                if fetch {
                    Shape::Fetch
                } else {
                    Shape::Read
                }
            }
            None if cpu.is_halted() => Shape::Fetch,
            None => Shape::Internal,
        };
        let address = first.map_or(cpu.regs().pc, |a| a.address);
        let value = first.map_or(0, |a| a.value);
        let sample = CycleSample {
            address,
            m1: shape == Shape::Fetch,
            write: shape == Shape::Write,
            value,
            known_mask: 255,
        };
        let physical = cpu.next_physical_cycle(&state).ok();
        let duration = physical
            .as_ref()
            .and_then(|r| r.sampled_t_states(sample).ok());
        if let Some(duration) = duration
            && duration > instruction.cycles
        {
            let (_, folded_state, folded, fold_accesses) =
                run(cpu, state, memory, audio, tick, irq, forced, 2)?;
            ensure!(
                folded.cycles == duration && fold_accesses == accesses,
                "unqualified SOUND extended cycle fold"
            );
            steps = 2;
            instruction = folded;
            accesses = fold_accesses;
            after_state = folded_state;
        }
        let external = shape != Shape::Internal && external(address);
        if external {
            let kind = physical
                .ok_or_else(|| anyhow::anyhow!("unqualified external SOUND cycle"))?
                .kind();
            ensure!(
                matches!(
                    kind,
                    CycleKind::MemoryRead { .. } | CycleKind::MemoryWrite { .. }
                ) && kind.address() == address
                    && kind.is_write() == (shape == Shape::Write)
                    && instruction.cycles == 3
                    && steps == 1
                    && accesses.len() == 1,
                "external SOUND cycle is not one qualified3T memory transfer"
            );
        }
        let opaque_tail = match shape {
            Shape::Fetch => {
                ensure!(
                    (4..=6).contains(&instruction.cycles)
                        || instruction.cycles == 11 && value & 0xcf == 0x09,
                    "unqualified SOUND fetch aggregate duration"
                );
                instruction.cycles == 11
            }
            Shape::Read | Shape::Write => {
                ensure!(
                    instruction.cycles >= 3 && instruction.cycles <= 8,
                    "unqualified SOUND memory duration"
                );
                duration != Some(instruction.cycles) && instruction.cycles != 3
            }
            Shape::InterruptAck => unreachable!("typed interrupt plan"),
            Shape::Internal => {
                ensure!(
                    instruction.cycles > 0 && instruction.cycles <= 7,
                    "unqualified SOUND internal duration"
                );
                true
            }
        };
        let refresh = u16::from_be_bytes([cpu.regs().i, cpu.regs().r]);
        let bank = if external { memory.bank } else { 0 };
        let finishes = !after_state.pending();
        let plan = Self {
            shape,
            address,
            value,
            refresh,
            cycles: instruction.cycles,
            steps,
            external,
            bank,
            opaque_tail,
            irq,
            finishes,
            im1: None,
            jr: None,
            copy: None,
            word: None,
            djnz: None,
            indexed: None,
            ed_word: None,
            instruction,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(self) -> Result<()> {
        ensure!(
            u8::from(self.im1.is_some())
                + u8::from(self.jr.is_some())
                + u8::from(self.copy.is_some())
                + u8::from(self.word.is_some())
                + u8::from(self.djnz.is_some())
                + u8::from(self.indexed.is_some())
                + u8::from(self.ed_word.is_some())
                <= 1,
            "overlapping SOUND cycle bindings"
        );
        if let Some(phase) = self.ed_word {
            return self.validate_ed_word(phase);
        }
        if let Some(phase) = self.indexed {
            return self.validate_indexed(phase);
        }
        if let Some(phase) = self.djnz {
            return self.validate_djnz(phase);
        }
        if let Some(phase) = self.word {
            return self.validate_word(phase);
        }
        if let Some(phase) = self.copy {
            ensure!(
                self.im1.is_none()
                    && self.jr.is_none()
                    && !self.opaque_tail
                    && self.steps == 1
                    && match phase {
                        CopyPhase::Read =>
                            self.shape == Shape::Read
                                && self.cycles == 3
                                && self.value == 0
                                && !self.finishes
                                && if self.external {
                                    self.address >= 0x8000 && self.bank < 0x80
                                } else {
                                    self.address < 0x4000
                                },
                        CopyPhase::Write =>
                            self.shape == Shape::Write
                                && self.cycles == 5
                                && !self.external
                                && self.address < 0x4000,
                        CopyPhase::Repeat =>
                            self.shape == Shape::Internal
                                && self.cycles == 5
                                && self.value == 0
                                && self.finishes
                                && !self.external
                                && self.address < 0x4000,
                    },
                "invalid physical copy plan"
            );
        }
        if let Some(phase) = self.jr {
            ensure!(
                self.im1.is_none()
                    && !self.opaque_tail
                    && self.steps == 1
                    && self.value == 0
                    && match phase {
                        JrPhase::Read => self.shape == Shape::Read && self.cycles == 3,
                        JrPhase::Internal =>
                            self.shape == Shape::Internal
                                && self.cycles == 5
                                && self.finishes
                                && !self.external
                                && !self.irq,
                    },
                "invalid typed JR plan"
            );
        }
        ensure!(
            if self.im1.is_some() {
                self.steps == 1
                    && !self.opaque_tail
                    && self.instruction.opcode == zeff_z80::Z80_INTERRUPT_ACK_OPCODE
                    && (self.shape == Shape::InterruptAck
                        && self.cycles == 7
                        && !self.finishes
                        && !self.external
                        && self.value == 0
                        && self.irq
                        || self.shape == Shape::Write && self.cycles == 3)
            } else {
                self.shape != Shape::InterruptAck
            },
            "invalid typed IM1 plan"
        );
        ensure!(
            self.steps > 0
                && self.steps <= 2
                && self.cycles == self.instruction.cycles
                && self.cycles > 0
                && self.cycles <= 32,
            "invalid SOUND plan span"
        );
        ensure!(
            !self.external
                || external(self.address)
                    && self.cycles == 3
                    && self.steps == 1
                    && matches!(self.shape, Shape::Read | Shape::Write),
            "invalid external SOUND plan"
        );
        ensure!(
            self.external
                || matches!(self.shape, Shape::Internal | Shape::InterruptAck)
                || !external(self.address),
            "external SOUND address marked local"
        );
        ensure!(
            self.bank < 512 && (self.external || self.bank == 0),
            "invalid captured SOUND bank"
        );
        Ok(())
    }
    pub fn validate_binding(self, cpu: &Cpu, state: BusState, memory: &SoundMemory) -> Result<()> {
        ensure!(
            self.jr != Some(JrPhase::Internal),
            "JR internal validation requires its tail"
        );
        let forced =
            matches!(self.shape, Shape::Fetch | Shape::Read).then_some((self.address, self.value));
        let expected = Self::prepare_inner(cpu, state, memory, None, 0, self.irq, forced)?;
        ensure!(
            self == expected,
            "SOUND plan differs from semantic continuation"
        );
        Ok(())
    }
}
