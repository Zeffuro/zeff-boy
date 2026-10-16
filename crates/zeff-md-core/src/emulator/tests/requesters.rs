use super::*;
use crate::hardware::bus::ownership::{Requester, Reservation};
use zeff_m68k::bus::{AccessKind, BusAccess, Direction};
use zeff_m68k::timing::{ClockedBus, CycleResult};

const THIRD: Requester = Requester(2);
const FOREIGN_START: u64 = 105;
const FOREIGN_END: u64 = 165;

struct ReservedBus<'a> {
    inner: ScheduledBus<'a>,
    requester: Option<Requester>,
    events: Vec<(u32, Direction, u16, u64, u32)>,
}

impl ReservedBus<'_> {
    fn record(&mut self, access: BusAccess, direction: Direction, value: u16, waits: u32) {
        self.events.push((
            access.bus_address,
            direction,
            value,
            self.inner.bus.now.get(),
            waits,
        ));
        if access.kind == AccessKind::Data
            && let Some(requester) = self.requester.take()
        {
            assert_eq!(self.inner.bus.now.get(), 84);
            self.inner
                .schedule
                .pending
                .as_mut()
                .unwrap()
                .reserve_after(&[Reservation {
                    requester,
                    ticks: FOREIGN_START..FOREIGN_END,
                }])
                .unwrap();
        }
    }
}

impl ClockedBus for ReservedBus<'_> {
    type Error = BusError;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        let result = self.inner.read_cycle(access, start);
        if let Ok(value) = result.result {
            self.record(access, Direction::Read, value, result.wait_clocks);
        }
        result
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        let result = self.inner.write_cycle(access, value, start);
        if result.result.is_ok() {
            self.record(access, Direction::Write, value, result.wait_clocks);
        }
        result
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.inner.idle(start, clocks);
    }
}

fn run(
    machine: &mut Emulator,
    requester: Requester,
    clocks: u64,
) -> Vec<(u32, Direction, u16, u64, u32)> {
    let mut bus = ReservedBus {
        inner: ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        },
        requester: Some(requester),
        events: Vec::new(),
    };
    assert_eq!(machine.cpu.step(&mut bus).unwrap().clocks, clocks);
    assert!(bus.requester.is_none());
    bus.events
}

#[test]
fn foreign_reservation_preserves_real_cpu_long_halves_and_clones() {
    for requester in [THIRD, Requester::SOUND] {
        let mut machine = super::waits::fixture(0x2080);
        let mut copied = machine.clone();
        let (clocks, second, refill, waits, sound) = if requester == THIRD {
            (37, 238, 315, [18, 7], 29)
        } else {
            (29, 168, 259, [8, 9], 25)
        };
        let events = run(&mut machine, requester, clocks);
        assert_eq!(
            events,
            [
                (0xff8010, Direction::Write, 0xa1b2, 84, 0),
                (0xff8012, Direction::Write, 0xc3d4, second, waits[0]),
                (0x104, Direction::Read, 0x4e71, refill, waits[1]),
            ]
        );
        assert_eq!(run(&mut copied, requester, clocks), events);
        assert_eq!(machine, copied);
        assert_eq!(machine.cpu.clock(), CpuClocks(8 + clocks));
        assert_eq!(machine.sound_cpu.cycles(), sound);
        assert_eq!(machine.sound_cpu.regs().sp, 0x8000);
        assert_eq!(machine.cpu.registers.pc, 0x102);
        assert_eq!(&machine.bus.ram[0x8010..0x8014], &[0xa1, 0xb2, 0xc3, 0xd4]);
        assert_eq!(&machine.bus.ram[0x8000..0x8002], &[0x34, 0x12]);
    }
    let mut machine = super::waits::fixture(0x2010);
    machine.cpu.registers.a[0] = 0xff7ffe;
    assert_eq!(
        run(&mut machine, THIRD, 37),
        [
            (0xff7ffe, Direction::Read, 0xdede, 84, 0),
            (0xff8000, Direction::Read, 0xde12, 238, 18),
            (0x104, Direction::Read, 0x4e71, 315, 7),
        ]
    );
    assert_eq!(machine.cpu.registers.d[0], 0xdedede12);
    assert_eq!(&machine.bus.ram[0x8000..0x8002], &[0x34, 0x12]);
}

#[test]
fn foreign_reservation_fault_preserves_publication_and_frozen_replay() {
    let mut machine = super::waits::fixture(0x2010);
    machine.cpu.registers.a[0] = 0x3ffffe;
    let mut bus = ReservedBus {
        inner: ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        },
        requester: Some(THIRD),
        events: Vec::new(),
    };
    assert!(matches!(
        machine.cpu.step(&mut bus),
        Err(TimingFault::Bus {
            source: BusError::UnsupportedAccess {
                address: 0x400000,
                direction: Direction::Read
            },
            ..
        })
    ));
    assert_eq!(bus.events, [(0x3ffffe, Direction::Read, 0xffff, 84, 0)]);
    assert_eq!(machine.bus.now.get(), 238);
    assert_eq!(machine.cpu.clock(), CpuClocks(34));
    assert_eq!(machine.bus.cpu_clock, CpuClocks(34));
    assert_eq!(machine.sound_cpu.cycles(), 24);
    assert_eq!(machine.sound_deadline(), Some(MasterTicks::new(285)));
    assert_eq!(machine.cpu.registers.d[0], 0xa1b2c3d4);
    assert_eq!(&machine.bus.ram[0x8000..0x8002], &[0xde, 0x12]);
    let before = machine.clone();
    assert_eq!(
        machine.step_instruction(),
        Err(MachineFault::Main(TimingFault::Frozen))
    );
    assert_eq!(machine, before);
}

#[test]
fn foreign_reservation_overflow_leaves_owned_phase_unchanged() {
    let mut machine = super::waits::fixture(0x2080);
    ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .advance_machine(20)
    .unwrap();
    let before = machine.clone();
    let pending = machine.sound_schedule.pending.as_mut().unwrap();
    assert_eq!(pending.reservation().unwrap().requester, Requester::SOUND);
    assert_eq!(
        pending.reserve_after(&[Reservation {
            requester: THIRD,
            ticks: 75..u64::MAX
        }]),
        Err(SoundFault::ClockOverflow)
    );
    assert_eq!(machine, before);
}
