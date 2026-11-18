use super::*;
use crate::timing::test_bus::Bus;
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pins {
    bus: Bus,
    from: u8,
    to: u8,
    edge: u64,
    wait_address: Option<u32>,
}

impl Pins {
    fn level(&self, half: u64) -> u8 {
        if half < 20 {
            0
        } else if half < self.edge {
            self.from
        } else {
            self.to
        }
    }
    fn samples(&self, start: CpuClocks, role: InterruptAperture) -> Option<InterruptSamples> {
        let offsets = match role {
            InterruptAperture::FollowingPrefetch => [2, 4],
            InterruptAperture::HandlerRefill => [6, 8],
        };
        InterruptSamples::new(
            self.level(start.0 * 2 + offsets[0]),
            self.level(start.0 * 2 + offsets[1]),
        )
    }
    fn step(&mut self, cpu: &mut PrefetchCpu) -> Result<TimedOutcome, TimingFault<usize>> {
        let level = self.level(cpu.clock().0 * 2);
        cpu.interrupt_input().set_level(level);
        cpu.step_boundary(self)
    }
}

impl ClockedBus for Pins {
    type Error = usize;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        self.bus.wait = u32::from(self.wait_address == Some(access.bus_address));
        self.bus.read_cycle(access, start)
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        self.bus.wait = u32::from(self.wait_address == Some(access.bus_address));
        self.bus.write_cycle(access, value, start)
    }
    fn read_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        role: InterruptAperture,
    ) -> ObservedCycle<u16, usize> {
        let samples = self.samples(start, role);
        let cycle = self.read_cycle(access, start);
        ObservedCycle { cycle, samples }
    }
    fn write_cycle_observed(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> ObservedCycle<(), usize> {
        let samples = self.samples(start, InterruptAperture::FollowingPrefetch);
        let cycle = self.write_cycle(access, value, start);
        ObservedCycle { cycle, samples }
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.bus.idle(start, clocks);
    }
    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, usize> {
        self.bus.wait = 0;
        self.bus.acknowledge_cycle(access, start)
    }
}

fn fixture(from: u8, to: u8, edge: u64) -> (PrefetchCpu, Pins) {
    let mut registers = Registers::new();
    registers.pc = 0x100;
    registers.ssp = 0x8000;
    registers.set_status_register(0x2000);
    let cpu = PrefetchCpu::from_prefetched(registers, [0x33fc, 0x64], CpuClocks(0)).unwrap();
    let mut bus = Bus::default();
    for (address, word) in [
        (0x104, 0),
        (0x106, 0x500),
        (0x108, 0x4ef9),
        (0x10a, 0),
        (0x10c, 0x200),
        (0x200, 0x4e71),
        (0x202, 0x4e71),
        (0x204, 0x4e71),
        (0x600, 0x4e71),
        (0x602, 0x4e71),
        (0x620, 0x4e71),
        (0x622, 0x4e71),
        (112, 0),
        (114, 0x600),
        (120, 0),
        (122, 0x620),
        (124, 0),
        (126, 0x620),
    ] {
        bus.words.insert(address, word);
    }
    (
        cpu,
        Pins {
            bus,
            from,
            to,
            edge,
            wait_address: None,
        },
    )
}

pub(in crate::timing) fn normal_capture_contract() {
    for (edge, saved_pc) in [(26, 0x108), (27, 0x200)] {
        let (mut cpu, mut bus) = fixture(0, 4, edge);
        assert!(matches!(
            bus.step(&mut cpu).unwrap(),
            TimedOutcome::Instruction(_)
        ));
        if edge == 27 {
            assert!(matches!(
                bus.step(&mut cpu).unwrap(),
                TimedOutcome::Instruction(_)
            ));
        }
        assert!(matches!(
            bus.step(&mut cpu).unwrap(),
            TimedOutcome::Interrupt { level: 4, .. }
        ));
        assert_eq!(bus.bus.words[&0x7ffe], saved_pc);
        assert_eq!(bus.bus.words[&0x500], 0x64);
    }
    let (mut cpu, mut bus) = fixture(4, 6, 29);
    bus.step(&mut cpu).unwrap();
    assert!(matches!(
        bus.step(&mut cpu).unwrap(),
        TimedOutcome::Interrupt { level: 4, .. }
    ));
    assert!(matches!(
        bus.step(&mut cpu).unwrap(),
        TimedOutcome::Interrupt { level: 6, .. }
    ));
    assert_eq!(bus.bus.words[&0x7ff4], 0x2400);
    assert_eq!(bus.bus.words[&0x7ff8], 0x600);
    let (mut cpu, mut bus) = fixture(6, 0, 29);
    bus.step(&mut cpu).unwrap();
    assert!(matches!(
        bus.step(&mut cpu).unwrap(),
        TimedOutcome::Interrupt { level: 6, .. }
    ));
    for address in [0x500, 0x10a] {
        let (mut cpu, mut bus) = fixture(0, 4, 27);
        bus.wait_address = Some(address);
        bus.step(&mut cpu).unwrap();
        assert_eq!(cpu.interrupts.observed_parts(), None);
        assert!(matches!(
            bus.step(&mut cpu).unwrap(),
            TimedOutcome::Interrupt { level: 4, .. }
        ));
    }
    for failure in 0..5 {
        let (mut cpu, mut bus) = fixture(0, 4, 26);
        bus.bus.fail = Some(failure);
        let registers = cpu.registers;
        assert!(matches!(bus.step(&mut cpu),Err(TimingFault::Bus {source,..}) if source==failure));
        assert_eq!(cpu.registers, registers);
        assert_eq!(cpu.clock().0, bus.bus.clock);
        let events = bus.bus.events.clone();
        assert_eq!(bus.step(&mut cpu), Err(TimingFault::Frozen));
        assert_eq!(events, bus.bus.events);
    }
    for failure in 0..8 {
        let (mut cpu, mut bus) = fixture(4, 6, 29);
        bus.step(&mut cpu).unwrap();
        let registers = cpu.registers;
        bus.bus.fail = Some(5 + failure);
        assert!(
            matches!(bus.step(&mut cpu),Err(TimingFault::Bus {source,..}) if source==5+failure)
        );
        assert_eq!(cpu.registers, registers);
        assert_eq!(cpu.clock().0, bus.bus.clock);
        let events = bus.bus.events.clone();
        assert_eq!(bus.step(&mut cpu), Err(TimingFault::Frozen));
        assert_eq!(events, bus.bus.events);
    }
    let (mut cpu, mut bus) = fixture(0, 4, 27);
    bus.step(&mut cpu).unwrap();
    cpu.interrupt_input().set_level(7);
    cpu.interrupt_input().set_level(0);
    assert_eq!(cpu.interrupts.observed_parts(), None);
    assert!(matches!(
        cpu.step_boundary(&mut bus).unwrap(),
        TimedOutcome::Interrupt { level: 7, .. }
    ));
    assert_eq!(cpu.interrupts.observed_parts(), None);
}

fn bytes(cpu: &PrefetchCpu) -> Vec<u8> {
    let mut writer = StateWriter::new();
    cpu.write_boundary_state(&mut writer).unwrap();
    writer.into_bytes()
}

pub(in crate::timing) fn sampled_state_contract() {
    for edge in [26, 27] {
        let (mut cpu, mut bus) = fixture(0, 4, edge);
        bus.step(&mut cpu).unwrap();
        let encoded = bytes(&cpu);
        assert_eq!(encoded[0], 2);
        let mut restored = PrefetchCpu::default();
        restored
            .read_boundary_state(&mut StateReader::new(&encoded))
            .unwrap();
        assert_eq!(cpu, restored);
        let mut other = bus.clone();
        for _ in 0..3 {
            assert_eq!(bus.step(&mut cpu), other.step(&mut restored));
            assert_eq!(cpu, restored);
            assert_eq!(bus, other);
        }
        for (pending, level) in [(2, 4), (1, 0), (0, 7)] {
            let mut bad = encoded.clone();
            let len = bad.len();
            bad[len - 2] = pending;
            bad[len - 1] = level;
            let old = restored.clone();
            assert!(
                restored
                    .read_boundary_state(&mut StateReader::new(&bad))
                    .is_err()
            );
            assert_eq!(restored, old);
        }
        let (mut legacy, mut pins) = fixture(0, 4, 27);
        pins.step(&mut legacy).unwrap();
        legacy.interrupts.clear_observation();
        let old = bytes(&legacy);
        assert_eq!(old[0], 1);
        restored
            .read_boundary_state(&mut StateReader::new(&old))
            .unwrap();
        assert_eq!(restored.interrupts.observed_parts(), None);
        assert!(matches!(
            pins.step(&mut restored).unwrap(),
            TimedOutcome::Interrupt { level: 4, .. }
        ));
    }
}
