use super::plan::{Memory, Plan, Step, sample};
use zeff_m68k::bus::BusAccess;
use zeff_m68k::timing::CpuClocks;
use zeff_m68k::timing::physical::{EntryBus, EntryOutcome, EntryReport};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionOutcome, InstructionReport,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub memory: Memory,
    pub steps: Vec<Step>,
    pub actual: Vec<Step>,
    pub clocks: u64,
    pub cursor: usize,
    pub error: Option<String>,
}

impl Device {
    pub fn new(plan: &Plan) -> Self {
        Self {
            memory: plan.memory.clone(),
            steps: plan.steps.clone(),
            actual: Vec::new(),
            clocks: 0,
            cursor: 0,
            error: None,
        }
    }

    fn read(&mut self, access: BusAccess, start: CpuClocks) -> Result<u16, String> {
        if self.error.is_some() || start.0 != self.clocks {
            return Err("read start/device state differs".into());
        }
        let value = sample(&self.memory, access.bus_address)?;
        let actual = Step::Read(access, value);
        self.actual.push(actual.clone());
        if self.steps.get(self.cursor) != Some(&actual) {
            return Err("actual logical/bus/FC/lanes/kind/data/order differs".into());
        }
        self.cursor += 1;
        self.clocks += 4;
        Ok(value)
    }
}

impl EntryBus for Device {
    type Error = String;
    fn read_entry(&mut self, _access: BusAccess, _start: CpuClocks) -> EntryReport<u16, String> {
        self.error = Some("unexpected entry read".into());
        EntryReport {
            elapsed: 0,
            outcome: EntryOutcome::HostFailure("unexpected entry read".into()),
        }
    }
    fn write_entry(
        &mut self,
        _access: BusAccess,
        _value: u16,
        _start: CpuClocks,
    ) -> EntryReport<(), String> {
        self.error = Some("unexpected entry write".into());
        EntryReport {
            elapsed: 0,
            outcome: EntryOutcome::HostFailure("unexpected entry write".into()),
        }
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        let actual = Step::Idle(clocks);
        self.actual.push(actual.clone());
        if self.error.is_some()
            || start.0 != self.clocks
            || self.steps.get(self.cursor) != Some(&actual)
        {
            self.error = Some("actual status I2 start/order/duration differs".into());
            return;
        }
        self.cursor += 1;
        self.clocks += u64::from(clocks);
    }
}

impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, String> {
        match self.read(access, start) {
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
        _access: BusAccess,
        _value: u16,
        _start: CpuClocks,
    ) -> InstructionReport<(), String> {
        self.error = Some("unexpected instruction write".into());
        InstructionReport {
            elapsed: 0,
            outcome: InstructionOutcome::HostFailure("unexpected instruction write".into()),
        }
    }
}
