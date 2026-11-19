use super::*;
use crate::hardware::bus::{Bus, BusError, scheduled::ScheduledBus};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult};

mod vdp_reads;

#[derive(Clone)]
pub(super) struct Machine {
    pub(super) bus: Bus,
    pub(super) cpu: Cpu,
    pub(super) schedule: SoundSchedule,
}

pub(super) fn access(address: u32) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes: ByteLanes::Both,
    }
}

impl Machine {
    pub(super) fn new(batched: bool) -> Self {
        let rig = Rig::new(&[0; 16], batched);
        let mut bus = Bus::new(rig.cartridge, crate::Region::NtscUs);
        bus.sound = rig.memory;
        bus.audio = rig.audio;
        Self {
            bus,
            cpu: rig.cpu,
            schedule: rig.schedule,
        }
    }

    pub(super) fn read(
        &mut self,
        address: u32,
        clock: Option<CpuClocks>,
    ) -> CycleResult<u16, BusError> {
        let clock = clock.unwrap_or(self.bus.cpu_clock);
        ScheduledBus::new(&mut self.bus, &mut self.cpu, &mut self.schedule)
            .read_cycle(access(address), clock)
    }

    fn write(&mut self, address: u32, value: u16) -> CycleResult<(), BusError> {
        let clock = self.bus.cpu_clock;
        ScheduledBus::new(&mut self.bus, &mut self.cpu, &mut self.schedule).write_cycle(
            access(address),
            value,
            clock,
        )
    }

    fn canonical(mut self) -> Self {
        self.schedule.finish_epoch_field(
            &mut self.cpu,
            &self.bus.sound,
            &mut crate::sound_trace::NoSoundCapture,
        );
        self
    }
}

fn equal_machine(actual: &Machine, expected: &Machine) {
    let actual = actual.clone().canonical();
    assert_eq!(actual.cpu, expected.cpu);
    assert_eq!(actual.schedule, expected.schedule);
    assert_eq!(actual.bus, expected.bus);
}

fn running_pair() -> (Machine, Machine) {
    let mut actual = Machine::new(true);
    let mut expected = Machine::new(false);
    for _ in 0..8 {
        assert_eq!(actual.read(8, None), expected.read(8, None));
    }
    assert!(actual.schedule.epoch_deadline().is_some());
    assert!(actual.cpu.cycles() < expected.cpu.cycles());
    equal_machine(&actual, &expected);
    (actual, expected)
}

#[test]
fn work_ram_write_is_independent_but_busreq_sound_ram_and_reset_materialize() {
    let (mut actual, mut expected) = running_pair();
    assert_eq!(
        actual.write(0xff0040, 0x1234),
        expected.write(0xff0040, 0x1234)
    );
    assert!(actual.schedule.epoch_deadline().is_some());
    equal_machine(&actual, &expected);
    for (address, value) in [
        (0xa11100, 0x0100),
        (0xa00000, 0x7600),
        (0xa11100, 0),
        (0xa11200, 0),
        (0xa11200, 0x0100),
    ] {
        assert_eq!(actual.write(address, value), expected.write(address, value));
        equal_machine(&actual, &expected);
    }
    assert_eq!(actual.bus.sound.ram[0], 0x76);
    assert_eq!(actual.read(8, None), expected.read(8, None));
    equal_machine(&actual, &expected);
}

#[test]
fn late_main_read_fault_preserves_completed_prefix_and_exact_pending() {
    let (mut actual, mut expected) = running_pair();
    let before = actual.bus.now;
    let a = actual.read(0x800000, None);
    let b = expected.read(0x800000, None);
    assert_eq!(a, b);
    assert!(matches!(a.result, Err(BusError::UnsupportedAccess { .. })));
    assert!(actual.bus.now > before);
    equal_machine(&actual, &expected);
}

#[test]
fn early_main_clock_fault_keeps_the_existing_pending_cut() {
    let (mut actual, mut expected) = running_pair();
    let before = actual.bus.now;
    let a = actual.read(8, Some(CpuClocks(0)));
    let b = expected.read(8, Some(CpuClocks(0)));
    assert_eq!(a, b);
    assert!(matches!(a.result, Err(BusError::ClockMismatch { .. })));
    assert_eq!(actual.bus.now, before);
    equal_machine(&actual, &expected);
}
