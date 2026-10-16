use super::plan::{Memory, Plan, Step, deposit, memory, sample};
use zeff_m68k::bus::{BusAccess, Direction};
use zeff_m68k::timing::CpuClocks;
use zeff_m68k::timing::physical::{EntryBus, EntryOutcome, EntryReport};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionOutcome, InstructionReport,
};

pub(super) struct Device<'a> {
    pub memory: Memory,
    pub cursor: usize,
    pub elapsed: u64,
    pub idle_remaining: u32,
    pub error: Option<String>,
    plan: &'a Plan,
}

impl<'a> Device<'a> {
    pub fn new(plan: &'a Plan, bytes: &[(u32, u8)]) -> Result<Self, String> {
        Ok(Self {
            memory: memory(bytes)?,
            cursor: 0,
            elapsed: 0,
            idle_remaining: 0,
            error: None,
            plan,
        })
    }

    pub(super) fn transfer(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        value: Option<u16>,
        entry: bool,
    ) -> Result<u16, String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if start.0 != self.elapsed || self.idle_remaining != 0 {
            return Err("device transfer start/idle differs".into());
        }
        let Some(Step::Transfer {
            access: expected,
            direction,
            value: data,
            entry: expected_entry,
        }) = self.plan.steps.get(self.cursor)
        else {
            return Err("unexpected device transfer".into());
        };
        if access != *expected
            || entry != *expected_entry
            || value.is_some() != (*direction == Direction::Write)
        {
            return Err("device logical/bus/FC/lanes/kind/direction differs".into());
        }
        let result = if let Some(value) = value {
            if value != *data {
                return Err("CPU write data differs from manual/raw value".into());
            }
            deposit(&mut self.memory, access.bus_address, value);
            value
        } else {
            let actual = sample(&self.memory, access.bus_address)?;
            if actual != *data {
                return Err("actual RAM read differs from manual/raw value".into());
            }
            actual
        };
        self.cursor += 1;
        self.elapsed += 4;
        Ok(result)
    }

    pub(super) fn idle(&mut self, start: CpuClocks, clocks: u32) -> Result<(), String> {
        if start.0 != self.elapsed || clocks == 0 {
            return Err("device idle start/duration differs".into());
        }
        if self.idle_remaining == 0 {
            let Some(Step::Idle(n)) = self.plan.steps.get(self.cursor) else {
                return Err("unexpected device idle".into());
            };
            self.idle_remaining = *n;
        }
        if clocks > self.idle_remaining {
            return Err("device idle crosses ordered transfer".into());
        }
        self.idle_remaining -= clocks;
        self.elapsed += u64::from(clocks);
        if self.idle_remaining == 0 {
            self.cursor += 1;
        }
        Ok(())
    }
}

impl EntryBus for Device<'_> {
    type Error = String;

    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, String> {
        match self.transfer(access, start, None, true) {
            Ok(value) => EntryReport {
                elapsed: 4,
                outcome: EntryOutcome::Complete(value),
            },
            Err(error) => EntryReport {
                elapsed: 0,
                outcome: EntryOutcome::HostFailure(error),
            },
        }
    }

    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), String> {
        match self.transfer(access, start, Some(value), true) {
            Ok(_) => EntryReport {
                elapsed: 4,
                outcome: EntryOutcome::Complete(()),
            },
            Err(error) => EntryReport {
                elapsed: 0,
                outcome: EntryOutcome::HostFailure(error),
            },
        }
    }

    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        if self.error.is_none()
            && let Err(error) = self.idle(start, clocks)
        {
            self.error = Some(error);
        }
    }
}

impl InstructionBus for Device<'_> {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, String> {
        match self.transfer(access, start, None, false) {
            Ok(value) => InstructionReport {
                elapsed: 4,
                outcome: InstructionOutcome::Complete(value),
            },
            Err(error) => InstructionReport {
                elapsed: 0,
                outcome: InstructionOutcome::HostFailure(error),
            },
        }
    }

    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), String> {
        match self.transfer(access, start, Some(value), false) {
            Ok(_) => InstructionReport {
                elapsed: 4,
                outcome: InstructionOutcome::Complete(()),
            },
            Err(error) => InstructionReport {
                elapsed: 0,
                outcome: InstructionOutcome::HostFailure(error),
            },
        }
    }

    fn detect_instruction_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        start: CpuClocks,
    ) -> InstructionReport<(), String> {
        if self.idle_remaining != 0
            || start.0 != self.elapsed
            || direction != Direction::Read
            || !matches!(self.plan.steps.get(self.cursor), Some(Step::Detect(a)) if *a == access)
        {
            return InstructionReport {
                elapsed: 0,
                outcome: InstructionOutcome::HostFailure(
                    "unexpected odd detector access/start".into(),
                ),
            };
        }
        self.cursor += 1;
        self.elapsed += 8;
        InstructionReport {
            elapsed: 8,
            outcome: InstructionOutcome::AtGroup0Entry,
        }
    }
}
