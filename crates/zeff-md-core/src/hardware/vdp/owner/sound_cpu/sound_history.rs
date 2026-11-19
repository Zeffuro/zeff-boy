use super::super::super::sound_bus::{Phase, SoundBus};
use super::super::super::sound_clock::Edge;
use super::super::super::sound_semantics::{Plan, Shape};
use super::{M1Cells, RunningM1};
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};
use zeff_z80::{BusState, Cpu};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum CycleOrigin {
    Reset(bool),
    Memory { address: u16, write: bool },
    Fetch(u16),
}
impl CycleOrigin {
    fn cells(self) -> M1Cells {
        match self {
            Self::Reset(wait) => M1Cells::released(wait),
            Self::Memory { address, write } => M1Cells::memory_boundary(address, write),
            Self::Fetch(address) => M1Cells::boundary(address),
        }
    }
    fn write_state(self, w: &mut StateWriter) {
        match self {
            Self::Reset(wait) => {
                w.write_u8(0);
                w.write_bool(wait);
            }
            Self::Memory { address, write } => {
                w.write_u8(1);
                w.write_u16(address);
                w.write_bool(write);
            }
            Self::Fetch(address) => {
                w.write_u8(2);
                w.write_u16(address);
            }
        }
    }
    fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        Ok(match r.read_u8()? {
            0 => Self::Reset(r.read_bool()?),
            1 => {
                let address = r.read_u16()?;
                ensure!(address < 0x4000, "invalid local memory origin");
                Self::Memory {
                    address,
                    write: r.read_bool()?,
                }
            }
            2 => Self::Fetch(r.read_u16()?),
            _ => anyhow::bail!("invalid M1 cycle origin"),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) enum M1History {
    Unknown,
    Released {
        wait: bool,
    },
    Memory {
        plan: Plan,
        stage: u8,
    },
    Fetch {
        plan: Plan,
        stage: u8,
        origin: CycleOrigin,
        cells: M1Cells,
    },
}
impl M1History {
    pub fn origin() -> Self {
        Self::Unknown
    }
    pub fn clear(&mut self) {
        *self = Self::Unknown;
    }
    pub fn release(&mut self, wait: bool) {
        *self = Self::Released { wait };
    }
    fn qualified(plan: Plan) -> bool {
        plan.shape == Shape::Fetch
            && !plan.external
            && !plan.opaque_tail
            && plan.cycles == 4
            && plan.steps == 1
            && plan.address < 0x4000
    }
    pub fn observe(
        &mut self,
        edge: Edge,
        bus: &SoundBus,
        wait: bool,
        requested: bool,
        memory_predecessor: bool,
    ) {
        if wait || requested {
            self.clear();
            return;
        }
        if edge == Edge::None {
            return;
        }
        if matches!(self, Self::Released { .. }) && bus.phase == Phase::ResetReady && !bus.high {
            return;
        }
        let Some(plan) = bus.plan else {
            self.clear();
            return;
        };
        if bus.waiting || bus.waits != 0 || !(1..=4).contains(&bus.t) {
            self.clear();
            return;
        }
        let stage = (bus.t * 2 - u32::from(bus.high)) as u8;
        if memory_predecessor {
            let advances = match self {
                Self::Memory {
                    plan: previous,
                    stage: old,
                } => *previous == plan && stage == *old + 1,
                _ => false,
            };
            if (stage == 1 && bus.phase == Phase::Active || advances)
                && stage <= 6
                && !bus.grant_pending
                && (stage < 6 || bus.phase == Phase::Ready)
            {
                *self = Self::Memory { plan, stage };
            } else {
                self.clear();
            }
            return;
        }
        if !Self::qualified(plan) {
            self.clear();
            return;
        }
        if stage == 1 && bus.phase == Phase::Active {
            let predecessor = match self {
                Self::Released { wait } => {
                    let origin = CycleOrigin::Reset(*wait);
                    Some((origin, origin.cells()))
                }
                Self::Memory { plan, stage: 6 } => {
                    let origin = CycleOrigin::Memory {
                        address: plan.address,
                        write: plan.shape == Shape::Write,
                    };
                    Some((origin, origin.cells()))
                }
                Self::Fetch {
                    stage: 8, cells, ..
                } => Some((CycleOrigin::Fetch(cells.address), cells.clone())),
                _ => None,
            };
            if let Some((origin, cells)) = predecessor {
                *self = Self::Fetch {
                    plan,
                    stage: 0,
                    origin,
                    cells,
                };
            } else {
                self.clear();
                return;
            }
        }
        if let Self::Fetch {
            plan: previous,
            stage: old,
            cells,
            ..
        } = self
            && *previous == plan
            && stage == *old + 1
            && stage <= 8
        {
            *old = stage;
            for _ in 0..2 {
                cells.settle(bus.high, false, false, plan.address, plan.refresh);
            }
            return;
        }
        self.clear();
    }
    pub fn begin(&self, bus: &SoundBus, plan: Plan) -> Result<RunningM1> {
        let Self::Fetch {
            stage: 8, cells, ..
        } = self
        else {
            anyhow::bail!("RESET lacks an observed ordinary M1 low boundary");
        };
        ensure!(
            bus.phase == Phase::Ready && !bus.high && *cells == M1Cells::boundary(bus.held_address),
            "M1 held boundary differs"
        );
        Ok(RunningM1 {
            cells: cells.clone(),
            plan,
            held_address: bus.held_address,
            cancelled: false,
            repeat_origin: false,
        })
    }
    pub fn validate(&self, bus: &SoundBus, cpu: &Cpu, state: BusState) -> Result<()> {
        match self {
            Self::Unknown => {}
            Self::Released { .. } => ensure!(
                bus.phase == Phase::ResetReady && !bus.high,
                "invalid released M1 history"
            ),
            Self::Memory { plan, stage } => {
                plan.validate()?;
                ensure!(
                    plan.memory_predecessor_shape()
                        && !cpu.interrupts_enabled()
                        && !cpu.interrupt_enable_pending()
                        && (1..=6).contains(stage),
                    "invalid memory predecessor history"
                );
                if *stage == 6 {
                    ensure!(
                        bus.phase == Phase::Ready
                            && !bus.high
                            && !state.pending()
                            && bus.held_address == plan.address,
                        "memory predecessor boundary differs"
                    );
                } else {
                    ensure!(
                        plan.reset_predecessor(cpu, state),
                        "memory predecessor identity differs"
                    );
                    Self::active(bus, *plan, *stage)?;
                }
            }
            Self::Fetch {
                plan,
                stage,
                origin,
                cells,
            } => {
                plan.validate()?;
                ensure!(
                    Self::qualified(*plan) && (1..=8).contains(stage),
                    "invalid M1 history cycle"
                );
                if *stage == 8 {
                    ensure!(
                        !bus.high
                            && matches!(bus.phase, Phase::Ready | Phase::GrantPending)
                            && bus.held_address == plan.refresh,
                        "M1 boundary history disagrees with bus"
                    );
                } else {
                    Self::active(bus, *plan, *stage)?;
                }
                let mut expected = origin.cells();
                for stage in 1..=*stage {
                    for _ in 0..2 {
                        expected.settle(stage % 2 != 0, false, false, plan.address, plan.refresh);
                    }
                }
                ensure!(expected == *cells, "M1 aperture history differs");
            }
        }
        Ok(())
    }
    fn active(bus: &SoundBus, plan: Plan, stage: u8) -> Result<()> {
        ensure!(
            bus.phase == Phase::Active
                && bus.plan == Some(plan)
                && !bus.waiting
                && bus.waits == 0
                && u32::from(stage) == bus.t * 2 - u32::from(bus.high),
            "aperture history disagrees with active cycle"
        );
        Ok(())
    }
    pub fn boundary_data(&self) -> Option<Option<u8>> {
        match self {
            Self::Memory { plan, stage: 6 } => {
                Some((plan.shape == Shape::Write).then_some(plan.value))
            }
            _ => None,
        }
    }
    pub fn completed_write(&self) -> Option<(u16, u8)> {
        match self {
            Self::Memory { plan, stage: 6 } if plan.shape == Shape::Write => {
                Some((plan.address, plan.value))
            }
            _ => None,
        }
    }
    pub fn write_state(&self, w: &mut StateWriter) {
        match self {
            Self::Unknown => w.write_u8(0),
            Self::Released { wait } => {
                w.write_u8(1);
                w.write_bool(*wait);
            }
            Self::Memory { plan, stage } => {
                w.write_u8(2);
                plan.write_state(w);
                w.write_u8(*stage);
            }
            Self::Fetch {
                plan,
                stage,
                origin,
                cells,
            } => {
                w.write_u8(3);
                plan.write_state(w);
                w.write_u8(*stage);
                origin.write_state(w);
                cells.write_state(w);
            }
        }
    }
    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        Ok(match r.read_u8()? {
            0 => Self::Unknown,
            1 => Self::Released {
                wait: r.read_bool()?,
            },
            2 => Self::Memory {
                plan: Plan::read_state(r)?,
                stage: r.read_u8()?,
            },
            3 => Self::Fetch {
                plan: Plan::read_state(r)?,
                stage: r.read_u8()?,
                origin: CycleOrigin::read_state(r)?,
                cells: M1Cells::read_state(r)?,
            },
            _ => anyhow::bail!("invalid SOUND aperture history tag"),
        })
    }
}
