use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub count: usize,
    pub entry_count: usize,
    pub fault_at: usize,
}

impl Device {
    fn sample(&self, a: BusAccess) -> u16 {
        self.words.get(&a.bus_address).copied().unwrap_or(0x4e71)
    }
    fn record(&mut self, tag: u8, a: BusAccess, c: CpuClocks, value: u16) {
        assert_eq!(self.clock, c.0);
        self.events.push(Event {
            tag,
            clock: c.0,
            access: a,
            value,
        });
    }
}

impl EntryBus for Device {
    type Error = &'static str;
    fn read_entry(&mut self, a: BusAccess, c: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.record(1, a, c, 0);
        self.entry_count += 1;
        self.clock += 4;
        EntryReport {
            elapsed: 4,
            outcome: EntryOutcome::Complete(self.sample(a)),
        }
    }
    fn write_entry(&mut self, a: BusAccess, v: u16, c: CpuClocks) -> EntryReport<(), Self::Error> {
        self.record(2, a, c, v);
        self.words.insert(a.bus_address, v);
        self.entry_count += 1;
        self.clock += 4;
        EntryReport {
            elapsed: 4,
            outcome: EntryOutcome::Complete(()),
        }
    }
    fn idle_entry(&mut self, c: CpuClocks, n: u32) {
        let a = BusAccess {
            logical_address: 0,
            bus_address: 0,
            function_code: FunctionCode::UserData,
            kind: AccessKind::Prefetch,
            lanes: ByteLanes::Both,
        };
        self.record(3, a, c, n.try_into().unwrap());
        self.clock += u64::from(n);
    }
    fn detect_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        assert_eq!(d, Direction::Write);
        self.record(4, a, c, 0);
        EntryReport {
            elapsed: 0,
            outcome: EntryOutcome::TimingUnavailable,
        }
    }
}

impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        a: BusAccess,
        c: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.record(0, a, c, 0);
        let at = self.count;
        self.count += 1;
        let elapsed = [7, 11, 5][at];
        self.clock += u64::from(elapsed);
        InstructionReport {
            elapsed,
            outcome: if at == self.fault_at {
                InstructionOutcome::AtGroup0Entry
            } else {
                InstructionOutcome::Complete(self.sample(a))
            },
        }
    }
    fn write_instruction(
        &mut self,
        _: BusAccess,
        _: u16,
        _: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        panic!("unexpected compare write")
    }
}

impl ClockedBus for Device {
    type Error = &'static str;
    fn read_cycle(&mut self, _: BusAccess, _: CpuClocks) -> CycleResult<u16, Self::Error> {
        panic!("unexpected legacy read")
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), Self::Error> {
        panic!("unexpected legacy write")
    }
    fn idle(&mut self, _: CpuClocks, _: u32) {
        panic!("unexpected legacy idle")
    }
}
