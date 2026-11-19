use super::super::sound_semantics::CopyPhase;
use super::m1::{M1Cells, RunningM1};
use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CopyHistory {
    Unknown,
    Write {
        plan: Plan,
        stage: u8,
    },
    Repeat {
        write: Plan,
        plan: Plan,
        stage: u8,
        cells: M1Cells,
        cpu: Cpu,
        state: BusState,
    },
}
impl CopyHistory {
    pub fn clear(&mut self) {
        *self = Self::Unknown;
    }
    pub fn observe(
        &mut self,
        edge: Edge,
        bus: &SoundBus,
        cpu: &Cpu,
        state: BusState,
        blocked: bool,
    ) {
        if blocked
            || cpu.interrupts_enabled()
            || cpu.saved_interrupts_enabled()
            || cpu.interrupt_enable_pending()
        {
            self.clear();
            return;
        }
        if edge == Edge::None {
            return;
        }
        let Some(plan) = bus.plan else {
            self.clear();
            return;
        };
        if bus.waiting || bus.waits != 0 || bus.grant_pending || !(1..=5).contains(&bus.t) {
            self.clear();
            return;
        }
        let stage = (bus.t * 2 - u32::from(bus.high)) as u8;
        if plan.copy == Some(CopyPhase::Write) && !plan.finishes {
            let advances = matches!(self, Self::Write { plan: old, stage: s } if *old == plan && stage == *s + 1);
            if stage == 1 && bus.phase == Phase::Active
                || advances && (stage < 10 || bus.phase == Phase::Ready)
            {
                *self = Self::Write { plan, stage };
            } else {
                self.clear();
            }
            return;
        }
        if plan.copy != Some(CopyPhase::Repeat) {
            self.clear();
            return;
        }
        if stage == 1
            && bus.phase == Phase::Active
            && let Self::Write {
                plan: write,
                stage: 10,
            } = self
            && write.address == plan.address
            && write.instruction.pc == plan.instruction.pc
        {
            *self = Self::Repeat {
                write: *write,
                plan,
                stage: 0,
                cells: M1Cells::copy_write_boundary(plan.address),
                cpu: cpu.clone(),
                state,
            };
        }
        if let Self::Repeat {
            plan: old,
            stage: previous,
            cells,
            cpu: before,
            state: saved,
            ..
        } = self
            && *old == plan
            && *before == *cpu
            && *saved == state
            && stage == *previous + 1
            && stage <= 10
            && (stage < 10 || bus.phase == Phase::Ready)
        {
            for _ in 0..2 {
                cells.settle_repeat(bus.high, stage >= 9);
            }
            *previous = stage;
            return;
        }
        self.clear();
    }
    pub fn late_entry(&self, bus: &SoundBus, cpu: &Cpu, state: BusState) -> bool {
        matches!(self, Self::Repeat { plan, stage: 9, cpu: before, state: saved, .. }
            if bus.phase == Phase::Active && bus.high && bus.t == 5 && bus.plan == Some(*plan)
                && !bus.grant_pending && !bus.waiting && *before == *cpu && *saved == state)
    }
    pub fn running(&self, plan: Plan) -> Result<RunningM1> {
        let Self::Repeat {
            stage: 10, cells, ..
        } = self
        else {
            anyhow::bail!("late RESET lost completed repeat witness")
        };
        RunningM1::after_repeat(plan, cells.clone())
    }
    pub fn validate(
        &self,
        bus: &SoundBus,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        reset: Option<&SoundReset>,
    ) -> Result<()> {
        match self {
            Self::Unknown => ensure!(
                reset.is_none_or(|r| !r.waiting_repeat() && !r.is_repeat_origin()),
                "repeat RESET lacks history"
            ),
            Self::Write { plan, stage } => {
                plan.validate()?;
                ensure!(
                    plan.copy == Some(CopyPhase::Write)
                        && !plan.finishes
                        && (1..=10).contains(stage)
                        && reset.is_none()
                        && !cpu.interrupts_enabled()
                        && !cpu.saved_interrupts_enabled()
                        && !cpu.interrupt_enable_pending(),
                    "invalid copy write witness"
                );
                if *stage == 10 {
                    ensure!(
                        bus.phase == Phase::Ready && !bus.high && bus.held_address == plan.address,
                        "copy write boundary differs"
                    );
                    let next = Plan::prepare_copy(cpu, state, memory, false)?;
                    ensure!(
                        cpu.next_copy_cycle(&state)
                            .map_err(|e| anyhow::anyhow!("copy write handoff: {e:?}"))?
                            .captured_byte()
                            == Some(plan.value),
                        "copy write lost committed byte identity"
                    );
                    ensure!(
                        next.copy == Some(CopyPhase::Repeat)
                            && next.address == plan.address
                            && next.instruction.pc == plan.instruction.pc,
                        "copy write lost repeat continuation"
                    );
                } else {
                    ensure!(
                        bus.plan == Some(*plan)
                            && bus.phase == Phase::Active
                            && u32::from(*stage) == bus.t * 2 - u32::from(bus.high),
                        "copy write phase differs"
                    );
                    plan.validate_binding(cpu, state, memory)?;
                }
                if *stage >= 6 {
                    ensure!(
                        memory.read(plan.address) == Some(plan.value),
                        "copy write receipt differs"
                    );
                }
            }
            Self::Repeat {
                write,
                plan,
                stage,
                cells,
                cpu: before,
                state: saved,
            } => {
                write.validate()?;
                plan.validate_binding(before, *saved, memory)?;
                ensure!(
                    before
                        .next_copy_cycle(saved)
                        .map_err(|e| anyhow::anyhow!("copy witness: {e:?}"))?
                        .captured_byte()
                        == Some(write.value),
                    "copy repeat lost committed byte identity"
                );
                ensure!(
                    write.copy == Some(CopyPhase::Write)
                        && !write.finishes
                        && plan.copy == Some(CopyPhase::Repeat)
                        && write.address == plan.address
                        && write.instruction.pc == plan.instruction.pc
                        && (1..=10).contains(stage)
                        && !before.interrupts_enabled()
                        && !before.saved_interrupts_enabled()
                        && !before.interrupt_enable_pending()
                        && memory.read(write.address) == Some(write.value),
                    "invalid repeat witness"
                );
                let mut expected = M1Cells::copy_write_boundary(plan.address);
                for s in 1..=*stage {
                    for _ in 0..2 {
                        expected.settle_repeat(s % 2 != 0, s >= 9);
                    }
                }
                ensure!(*cells == expected, "copy repeat apertures differ");
                if *stage < 10 {
                    ensure!(
                        cells.pins() == bus.pins(false),
                        "repeat pin projection differs"
                    );
                    ensure!(
                        *before == *cpu
                            && *saved == state
                            && bus.phase == Phase::Active
                            && bus.plan == Some(*plan)
                            && u32::from(*stage) == bus.t * 2 - u32::from(bus.high),
                        "copy repeat phase differs"
                    );
                    ensure!(
                        reset.is_none_or(|r| r.waiting_repeat() && *stage == 9),
                        "RESET before final repeat high"
                    );
                } else {
                    let (after, after_state) = plan.retire_copy(before, *saved, memory, None, 0)?;
                    ensure!(
                        after == *cpu && after_state == state,
                        "completed copy repeat checkpoint differs"
                    );
                    ensure!(
                        reset.is_some_and(|r| r.repeat_origin_matches(cells)),
                        "completed repeat witness differs from RESET origin"
                    );
                }
            }
        }
        Ok(())
    }
    pub fn write_state(&self, w: &mut StateWriter) {
        match self {
            Self::Unknown => w.write_u8(0),
            Self::Write { plan, stage } => {
                w.write_u8(1);
                plan.write_state(w);
                w.write_u8(*stage);
            }
            Self::Repeat {
                write,
                plan,
                stage,
                cells,
                cpu,
                state,
            } => {
                w.write_u8(2);
                write.write_state(w);
                plan.write_state(w);
                w.write_u8(*stage);
                cells.write_state_copy(w);
                cpu.write_continuation(state, w);
            }
        }
    }
    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        Ok(match r.read_u8()? {
            0 => Self::Unknown,
            1 => Self::Write {
                plan: Plan::read_state(r)?,
                stage: r.read_u8()?,
            },
            2 => {
                let write = Plan::read_state(r)?;
                let plan = Plan::read_state(r)?;
                let stage = r.read_u8()?;
                let cells = M1Cells::read_state_copy(r)?;
                let mut cpu = Cpu::new();
                let mut state = BusState::default();
                cpu.read_continuation(&mut state, r)?;
                Self::Repeat {
                    write,
                    plan,
                    stage,
                    cells,
                    cpu,
                    state,
                }
            }
            _ => anyhow::bail!("invalid copy history"),
        })
    }
}
