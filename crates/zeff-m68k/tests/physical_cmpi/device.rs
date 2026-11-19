use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Host,
    Boundary,
    Unavailable,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u64),
    Write(BusAccess, u16, u64),
    Detect(BusAccess, Direction, u64),
    EntryRead(BusAccess, u64),
    EntryWrite(BusAccess, u16, u64),
    EntryDetect(BusAccess, Direction, u64),
    Idle(u64, u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device<const N: usize = 7> {
    pub clock: u64,
    pub events: Vec<Event>,
    pub words: Memory,
    pub count: usize,
    pub elapsed: [u32; N],
    pub failure: Option<(usize, Failure)>,
    pub detection: Option<(Failure, u32)>,
    pub entry_count: usize,
    pub entry_failure: Option<(usize, Failure)>,
    pub mutate_read: Option<(u32, u16)>,
}

impl<const N: usize> Device<N> {
    pub fn with_clock(clock: u64) -> Self {
        let mut device = Self {
            clock,
            events: Vec::new(),
            words: Memory::new(),
            count: 0,
            elapsed: [4; N],
            failure: None,
            detection: None,
            entry_count: 0,
            entry_failure: None,
            mutate_read: None,
        };
        for (address, value) in [
            (8, 0x4321),
            (10, 0x4000),
            (12, 0x4321),
            (14, 0x4000),
            (0x4321_4000, 0x4e71),
            (0x4321_4002, 0x7201),
        ] {
            device.word(address, value);
        }
        device
    }
    pub fn word(&mut self, address: u32, value: u16) {
        self.words.insert(address & BUS_ADDRESS_MASK & !1, value);
    }
    pub fn sample(&self, address: u32) -> u16 {
        self.words
            .get(&(address & BUS_ADDRESS_MASK & !1))
            .copied()
            .unwrap_or(0x4e71)
    }
    pub fn operand(&mut self, address: u32, size: Size, value: u32) {
        match size {
            Size::Byte => self.word(
                address,
                if address & 1 == 0 {
                    (value as u16 & 255) << 8 | 0x55
                } else {
                    0xaa00 | value as u16 & 255
                },
            ),
            Size::Word => self.word(address, value as u16),
            Size::Long => {
                self.word(address, (value >> 16) as u16);
                self.word(address.wrapping_add(2), value as u16);
            }
        }
    }
    fn start(&self, clock: CpuClocks) {
        assert_eq!(clock.0, self.clock, "device clock ownership");
    }
    fn apply(&mut self, access: BusAccess, value: u16) {
        let old = self.words.get(&access.bus_address).copied().unwrap_or(0);
        let mask = match access.lanes {
            ByteLanes::Both => 65535,
            ByteLanes::Upper => 0xff00,
            ByteLanes::Lower => 255,
        };
        self.words
            .insert(access.bus_address, (old & !mask) | (value & mask));
    }
    fn report<T>(&mut self, clock: CpuClocks, value: T) -> InstructionReport<T, &'static str> {
        self.start(clock);
        let index = self.count;
        self.count += 1;
        let failure = self
            .failure
            .filter(|(at, _)| *at == index)
            .map(|(_, failure)| failure);
        let elapsed = self.elapsed[index];
        self.clock += u64::from(elapsed);
        InstructionReport {
            elapsed,
            outcome: match failure {
                Some(Failure::Host) => InstructionOutcome::HostFailure("host"),
                Some(Failure::Boundary) => InstructionOutcome::AtGroup0Entry,
                Some(Failure::Unavailable) => InstructionOutcome::TimingUnavailable,
                Some(Failure::Complete) | None => InstructionOutcome::Complete(value),
            },
        }
    }
    fn entry_report<T>(&mut self, clock: CpuClocks, value: T) -> EntryReport<T, &'static str> {
        self.start(clock);
        let index = self.entry_count;
        self.entry_count += 1;
        self.clock += 4;
        let failure = self
            .entry_failure
            .filter(|(at, _)| *at == index)
            .map(|(_, failure)| failure);
        EntryReport {
            elapsed: 4,
            outcome: match failure {
                Some(Failure::Host) => EntryOutcome::HostFailure("host"),
                Some(Failure::Boundary) => EntryOutcome::CriticalAbortReleased,
                Some(Failure::Unavailable) => EntryOutcome::TimingUnavailable,
                Some(Failure::Complete) | None => EntryOutcome::Complete(value),
            },
        }
    }
}

impl<const N: usize> EntryBus for Device<N> {
    type Error = &'static str;
    fn read_entry(&mut self, access: BusAccess, clock: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.events.push(Event::EntryRead(access, clock.0));
        self.entry_report(clock, self.sample(access.bus_address))
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        clock: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.events.push(Event::EntryWrite(access, value, clock.0));
        self.apply(access, value);
        self.entry_report(clock, ())
    }
    fn idle_entry(&mut self, clock: CpuClocks, clocks: u32) {
        self.start(clock);
        self.events.push(Event::Idle(clock.0, clocks));
        self.clock += u64::from(clocks);
    }
    fn detect_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        clock: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.start(clock);
        self.events
            .push(Event::EntryDetect(access, direction, clock.0));
        EntryReport {
            elapsed: 0,
            outcome: EntryOutcome::TimingUnavailable,
        }
    }
}

impl<const N: usize> InstructionBus for Device<N> {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        clock: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        if let Some((address, value)) = self
            .mutate_read
            .filter(|(address, _)| *address == access.logical_address)
        {
            self.word(address, value);
            self.mutate_read = None;
        }
        self.events.push(Event::Read(access, clock.0));
        self.report(clock, self.sample(access.bus_address))
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        clock: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.events.push(Event::Write(access, value, clock.0));
        self.apply(access, value);
        self.report(clock, ())
    }
    fn detect_instruction_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        clock: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.start(clock);
        self.events.push(Event::Detect(access, direction, clock.0));
        let (failure, elapsed) = self.detection.unwrap_or((Failure::Unavailable, 0));
        self.clock += u64::from(elapsed);
        InstructionReport {
            elapsed,
            outcome: match failure {
                Failure::Host => InstructionOutcome::HostFailure("host"),
                Failure::Boundary => InstructionOutcome::AtGroup0Entry,
                Failure::Unavailable => InstructionOutcome::TimingUnavailable,
                Failure::Complete => InstructionOutcome::Complete(()),
            },
        }
    }
}

pub(super) struct DefaultDetection<'a, const N: usize = 5>(pub &'a mut Device<N>);
impl<const N: usize> EntryBus for DefaultDetection<'_, N> {
    type Error = &'static str;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.0.read_entry(access, start)
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.0.write_entry(access, value, start)
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.0.idle_entry(start, clocks);
    }
}
impl<const N: usize> InstructionBus for DefaultDetection<'_, N> {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.0.read_instruction(access, start)
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.0.write_instruction(access, value, start)
    }
}
impl<const N: usize> ClockedBus for Device<N> {
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

impl Device<7> {
    pub fn new(clock: u64) -> Self {
        Self::with_clock(clock)
    }
}
