use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Scripts {
    pub slots: [Option<(bool, Failure, u32)>; fixed::SLOTS],
    pub len: usize,
}
impl Default for Scripts {
    fn default() -> Self {
        Self {
            slots: [None; fixed::SLOTS],
            len: 0,
        }
    }
}
impl Scripts {
    pub fn push(&mut self, entry: bool, kind: Failure, elapsed: u32) {
        assert!(self.len < fixed::SLOTS, "script capacity");
        self.slots[self.len] = Some((entry, kind, elapsed));
        self.len += 1;
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub words: Ram,
    pub events: Events,
    pub scripts: Scripts,
    pub clock: u64,
    pub count: usize,
    pub entry_count: usize,
    pub elapsed: [u32; 7],
    pub entry_elapsed: [u32; 16],
    pub failure: Option<(usize, Failure)>,
    pub entry_failure: Option<(usize, Failure)>,
    pub secondary: Option<(Failure, u32)>,
    pub default_secondary: bool,
    pub mutation: Option<(u32, u16)>,
    pub mutations: usize,
}
impl Device {
    pub fn seed(c: Case) -> Self {
        let mut b = Self {
            words: Ram::default(),
            events: Events::default(),
            scripts: Scripts::default(),
            clock: c.start,
            count: 0,
            entry_count: 0,
            elapsed: [4; 7],
            entry_elapsed: [4; 16],
            failure: None,
            entry_failure: None,
            secondary: None,
            default_secondary: false,
            mutation: None,
            mutations: 0,
        };
        for (a, v) in [
            (8, 0x4321),
            (10, 0x4000),
            (12, 0x4321),
            (14, 0x4000),
            (0x4321_4000, 0x4e71),
            (0x4321_4002, 0x7201),
            (c.r.pc, c.opcode()),
            (c.r.pc.wrapping_add(2), c.retained()),
        ] {
            b.word(a, v);
        }
        if c.form.size == Size::Long {
            b.word(c.r.pc.wrapping_add(4), c.immediate as u16);
        }
        let anchor = c.r.pc.wrapping_add(c.form.stride());
        if c.form.extensions() > 0 {
            b.word(anchor.wrapping_add(2), c.extension);
        }
        if c.form.extensions() == 2 {
            b.word(anchor.wrapping_add(4), c.low);
        }
        b.word(c.r.pc.wrapping_add(c.form.length()), c.next);
        b.word(c.r.pc.wrapping_add(c.form.length() + 2), c.last);
        if c.form.ea != Ea::Dn {
            let address = super::super::oracle::address(c, &b.words.memory());
            b.word(
                address,
                if address & 1 == 0 {
                    ((c.value as u16 & 255) << 8) | 0x55
                } else {
                    0xaa00 | (c.value as u16 & 255)
                },
            );
        }
        b
    }
    pub fn word(&mut self, address: u32, value: u16) {
        self.words.insert(address, value);
    }
    fn start(&self, c: CpuClocks) {
        assert_eq!(self.clock, c.0, "device clock ownership");
    }
    fn report<T>(&mut self, c: CpuClocks, value: T) -> InstructionReport<T, &'static str> {
        self.start(c);
        let i = self.count;
        let kind = self
            .failure
            .filter(|(at, _)| *at == i)
            .map_or(Failure::Complete, |(_, k)| k);
        let elapsed = self.elapsed[i];
        self.count += 1;
        self.scripts.push(false, kind, elapsed);
        self.clock += u64::from(elapsed);
        InstructionReport {
            elapsed,
            outcome: match kind {
                Failure::Complete => InstructionOutcome::Complete(value),
                Failure::Host => InstructionOutcome::HostFailure("host"),
                Failure::Boundary => InstructionOutcome::AtGroup0Entry,
                Failure::Unavailable => InstructionOutcome::TimingUnavailable,
            },
        }
    }
    fn entry_report<T>(&mut self, c: CpuClocks, value: T) -> EntryReport<T, &'static str> {
        self.start(c);
        let i = self.entry_count;
        let kind = self
            .entry_failure
            .filter(|(at, _)| *at == i)
            .map_or(Failure::Complete, |(_, k)| k);
        let elapsed = self.entry_elapsed[i];
        self.entry_count += 1;
        self.scripts.push(true, kind, elapsed);
        self.clock += u64::from(elapsed);
        EntryReport {
            elapsed,
            outcome: match kind {
                Failure::Complete => EntryOutcome::Complete(value),
                Failure::Host => EntryOutcome::HostFailure("host"),
                Failure::Boundary => EntryOutcome::CriticalAbortReleased,
                Failure::Unavailable => EntryOutcome::TimingUnavailable,
            },
        }
    }
}
impl EntryBus for Device {
    type Error = &'static str;
    fn read_entry(&mut self, a: BusAccess, c: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.events.push(Event::EntryRead(a, c.0));
        self.entry_report(c, self.words.sample(a.bus_address))
    }
    fn write_entry(&mut self, a: BusAccess, v: u16, c: CpuClocks) -> EntryReport<(), Self::Error> {
        self.events.push(Event::EntryWrite(a, v, c.0));
        self.words.insert(a.bus_address, v);
        self.entry_report(c, ())
    }
    fn idle_entry(&mut self, c: CpuClocks, n: u32) {
        self.start(c);
        self.events.push(Event::Idle(c.0, n));
        self.clock += u64::from(n);
    }
    fn detect_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.start(c);
        self.events.push(Event::EntryDetect(a, d, c.0));
        let (kind, elapsed) = self.secondary.unwrap_or((Failure::Unavailable, 0));
        self.clock += u64::from(elapsed);
        EntryReport {
            elapsed,
            outcome: match kind {
                Failure::Complete => EntryOutcome::Complete(()),
                Failure::Host => EntryOutcome::HostFailure("host"),
                Failure::Boundary => EntryOutcome::CriticalAbortReleased,
                Failure::Unavailable => EntryOutcome::TimingUnavailable,
            },
        }
    }
}
impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        a: BusAccess,
        c: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        if let Some((address, value)) = self
            .mutation
            .filter(|(address, _)| *address == a.logical_address)
        {
            self.word(address, value);
            self.mutations += 1;
        }
        self.events.push(Event::Read(a, c.0));
        self.report(c, self.words.sample(a.bus_address))
    }
    fn write_instruction(
        &mut self,
        _: BusAccess,
        _: u16,
        _: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        panic!("CMPI writes no source")
    }
}
impl ClockedBus for Device {
    type Error = &'static str;
    fn read_cycle(&mut self, _: BusAccess, _: CpuClocks) -> CycleResult<u16, Self::Error> {
        panic!("legacy read")
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), Self::Error> {
        panic!("legacy write")
    }
    fn idle(&mut self, _: CpuClocks, _: u32) {
        panic!("legacy idle")
    }
}
pub(super) struct DefaultSecondary<'a>(pub &'a mut Device);
impl EntryBus for DefaultSecondary<'_> {
    type Error = &'static str;
    fn read_entry(&mut self, a: BusAccess, c: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.0.read_entry(a, c)
    }
    fn write_entry(&mut self, a: BusAccess, v: u16, c: CpuClocks) -> EntryReport<(), Self::Error> {
        self.0.write_entry(a, v, c)
    }
    fn idle_entry(&mut self, c: CpuClocks, n: u32) {
        self.0.idle_entry(c, n);
    }
}
impl InstructionBus for DefaultSecondary<'_> {
    fn read_instruction(
        &mut self,
        a: BusAccess,
        c: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.0.read_instruction(a, c)
    }
    fn write_instruction(
        &mut self,
        a: BusAccess,
        v: u16,
        c: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.0.write_instruction(a, v, c)
    }
}
