use super::*;
use crate::hardware::sound::CandidateBus;
use crate::{ControllerKind, ControllerPort};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CycleResult};
use zeff_z80::{Cpu, ResetState};

struct TraceBus<'a> {
    inner: ScheduledBus<'a>,
    events: Vec<(u32, Direction, u16, u64, u32)>,
}

impl ClockedBus for TraceBus<'_> {
    type Error = BusError;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        let result = self.inner.read_cycle(access, start);
        if let Ok(value) = result.result {
            self.events.push((
                access.bus_address,
                Direction::Read,
                value,
                self.inner.bus.now.get(),
                result.wait_clocks,
            ));
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
            self.events.push((
                access.bus_address,
                Direction::Write,
                value,
                self.inner.bus.now.get(),
                result.wait_clocks,
            ));
        }
        result
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.inner.idle(start, clocks);
    }
}

fn run(machine: &mut Emulator, clocks: u64) -> Vec<(u32, Direction, u16, u64, u32)> {
    let mut bus = TraceBus {
        inner: ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        },
        events: Vec::new(),
    };
    let step = machine.cpu.step(&mut bus).unwrap();
    assert_eq!(step.clocks, clocks);
    bus.events
}

#[test]
fn two_cpu_stack_reservations_preserve_each_long_half() {
    let mut machine = fixture(0x2080);
    assert_eq!(
        run(&mut machine, 29),
        [
            (0xff8010, Direction::Write, 0xa1b2, 84, 0),
            (0xff8012, Direction::Write, 0xc3d4, 168, 8),
            (0x104, Direction::Read, 0x4e71, 259, 9),
        ]
    );
    assert_eq!(&machine.bus.ram[0x8010..0x8014], &[0xa1, 0xb2, 0xc3, 0xd4]);
    assert_eq!(&machine.bus.ram[0x8000..0x8002], &[0x34, 0x12]);
    assert_eq!(machine.cpu.clock(), CpuClocks(37));
    assert_eq!(machine.sound_cpu.cycles(), 25);
    assert_eq!(machine.sound_cpu.regs().sp, 0x8000);
    assert_eq!(machine.cpu.registers.pc, 0x102);
    let mut machine = fixture(0x2010);
    machine.cpu.registers.a[0] = 0xff7ffe;
    assert_eq!(
        run(&mut machine, 29),
        [
            (0xff7ffe, Direction::Read, 0xdede, 84, 0),
            (0xff8000, Direction::Read, 0xde12, 168, 8),
            (0x104, Direction::Read, 0x4e71, 259, 9),
        ]
    );
    assert_eq!(machine.cpu.registers.d[0], 0xdedede12);
    assert_eq!(&machine.bus.ram[0x8000..0x8002], &[0x34, 0x12]);
}

#[test]
fn stack_reservation_lanes_clones_and_pending_reset() {
    for lower in [false, true] {
        let mut machine = fixture(0x1080);
        machine.cpu.registers.a[0] += u32::from(lower);
        machine.bus.irq_level = 2;
        machine
            .input_mut()
            .set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
        machine
            .input_mut()
            .set_controller_extra_raw(ControllerPort::One, 5);
        let mut copied = machine.clone();
        let events = run(&mut machine, 16);
        assert_eq!(
            events,
            [
                (
                    0xff8010,
                    Direction::Write,
                    if lower { 0xd4 } else { 0xd400 },
                    84,
                    0
                ),
                (0x104, Direction::Read, 0x4e71, 168, 8),
            ]
        );
        assert_eq!(run(&mut copied, 16), events);
        assert_eq!(machine, copied);
        assert_eq!(
            &machine.bus.ram[0x8010..0x8012],
            if lower { &[0xde, 0xd4] } else { &[0xd4, 0xde] }
        );
        assert_eq!(&machine.bus.ram[0x8000..0x8002], &[0xde, 0x12]);
        assert_eq!(machine.sound_deadline(), Some(MasterTicks::new(225)));
        assert_eq!(machine.peek(0xe08010), machine.peek(0xff8010));
        assert_eq!(machine.peek(0xe08011), machine.peek(0xff8011));
        machine.reset_sound_cpu();
        advance(&mut machine, 100);
        assert_eq!(&machine.bus.ram[0x8000..0x8002], &[0xde, 0x12]);
        assert_eq!(machine.input().controller_extra_raw(ControllerPort::One), 5);
        assert_eq!(machine.interrupt_level(), 2);
        copied.step_instruction().unwrap();
        assert_eq!(&copied.bus.ram[0x8000..0x8002], &[0x34, 0x12]);
    }
}

fn data_access(address: u32) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address,
        kind: AccessKind::Data,
        lanes: ByteLanes::Both,
        function_code: FunctionCode::SupervisorData,
    }
}

fn after_first_word() -> Emulator {
    let mut machine = fixture(0x2080);
    let result = ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .write_cycle(data_access(0xff8010), 0xa1b2, CpuClocks(8));
    assert_eq!(result.wait_clocks, 0);
    assert!(result.result.is_ok());
    assert_eq!(machine.sound_deadline(), Some(MasterTicks::new(135)));
    machine
}

#[test]
fn stack_preflight_and_mapped_fault_keep_elapsed_waits() {
    for kind in 0..4 {
        let mut machine = after_first_word();
        let mut start = machine.bus.cpu_clock;
        match kind {
            0 => start.0 += 1,
            1 => {
                machine.sound_schedule.pending.as_mut().unwrap().deadline =
                    84 + (u64::from(u32::MAX) + 1) * 7
            }
            2 => {
                machine.bus.now = MasterTicks::new(u64::MAX - 35);
                machine.sound_schedule.pending.as_mut().unwrap().deadline = u64::MAX;
            }
            3 => {
                machine.bus.cpu_clock = CpuClocks(u64::MAX - 4);
                start = machine.bus.cpu_clock;
            }
            _ => unreachable!(),
        }
        let saved = machine.clone();
        let result = ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .read_cycle(data_access(0xff8012), start);
        assert!(result.result.is_err());
        assert_eq!(result.wait_clocks, 0);
        assert_eq!(machine, saved);
    }
    let mut machine = after_first_word();
    let result = ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .read_cycle(data_access(0xa21100), CpuClocks(12));
    assert_eq!(
        result.result,
        Err(BusError::UnsupportedAccess {
            address: 0xa21100,
            direction: Direction::Read
        })
    );
    assert_eq!(result.wait_clocks, 8);
    assert_eq!(machine.bus.now.get(), 168);
    assert_eq!(machine.bus.cpu_clock, CpuClocks(24));
    assert_eq!(&machine.bus.ram[0x8010..0x8014], &[0xa1, 0xb2, 0xde, 0xde]);
    assert_eq!(&machine.bus.ram[0x8000..0x8002], &[0xde, 0x12]);
    let mut machine = fixture(0x2010);
    machine.cpu.registers.a[0] = 0x3ffffe;
    assert!(matches!(
        machine.step_instruction(),
        Err(MachineFault::Main(TimingFault::Bus {
            source: BusError::UnsupportedAccess {
                address: 0x400000,
                direction: Direction::Read
            },
            ..
        }))
    ));
    assert_eq!(machine.bus.now.get(), 168);
    assert_eq!(machine.bus.cpu_clock, CpuClocks(24));
    assert_eq!(machine.cpu.clock(), CpuClocks(24));
    assert_eq!(machine.cpu.registers.d[0], 0xa1b2c3d4);
    assert_eq!(&machine.bus.ram[0x8000..0x8002], &[0xde, 0x12]);
    let saved = machine.clone();
    assert_eq!(
        machine.step_instruction(),
        Err(MachineFault::Main(TimingFault::Frozen))
    );
    assert_eq!(machine, saved);
}
pub(super) fn fixture(opcode: u16) -> Emulator {
    let mut machine = super::machine(&[opcode, 0x4e71, 0x4e71]);
    machine.bus.ram.fill(0xde);
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x1ff;
    machine.bus.sound.ram[..5].copy_from_slice(&[0x21, 0x34, 0x12, 0xe5, 0x76]);
    machine.sound_cpu = Cpu::new_with_reset(ResetState::new(0, 0x8002));
    let mut snapshot = machine.bus.sound.clone();
    let mut setup = CandidateBus::new(&mut snapshot, &machine.bus.cartridge);
    machine.sound_cpu.step_with_bus(&mut setup).unwrap();
    assert_eq!(machine.sound_cpu.regs().hl(), 0x1234);
    assert_eq!(machine.sound_cpu.cycles(), 10);
    machine.bus.now = MasterTicks::ZERO;
    machine.bus.audio = crate::hardware::audio::Audio::default();
    machine.bus.vdp = Vdp::bootstrap_mode5();
    machine.bus.cpu_clock = CpuClocks(0);
    machine.cpu = PrefetchCpu::default();
    machine.cpu.registers = machine.bus.cartridge.initial;
    machine.cpu.registers.d[0] = 0xa1b2_c3d4;
    machine.cpu.registers.a[0] = 0xff8010;
    machine.concurrent_sound = true;
    machine
        .cpu
        .prime(&mut ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        })
        .unwrap();
    assert_eq!(machine.bus.now.get(), 56);
    assert_eq!(machine.cpu.clock(), CpuClocks(8));
    machine
}

fn advance(machine: &mut Emulator, ticks: u64) {
    ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .advance_machine(ticks)
    .unwrap();
}

#[test]
fn delayed_pending_deadlines_and_failure_are_atomic() {
    let mut machine = fixture(0x2080);
    advance(&mut machine, 19);
    assert_eq!(machine.bus.now.get(), 75);
    assert_eq!(machine.sound_cpu.cycles(), 15);
    assert!(machine.sound_schedule.pending.is_none());
    advance(&mut machine, 1);
    let high = machine.sound_schedule.pending.as_mut().unwrap();
    assert_eq!(high.deadline, 120);
    let saved = high.clone();
    high.extend_bus_segment(0).unwrap();
    assert_eq!(*high, saved);
    for waits in [u64::MAX, u64::from(u32::MAX) + 1, u64::from(u32::MAX) - 2] {
        assert_eq!(
            high.extend_bus_segment(waits),
            Err(crate::hardware::sound::SoundFault::ClockOverflow)
        );
        assert_eq!(*high, saved);
    }
    let mut edge = saved.clone();
    edge.deadline = u64::MAX - 14;
    let original = edge.clone();
    assert!(edge.extend_bus_segment(1).is_err());
    assert_eq!(edge, original);
    edge.deadline = u64::MAX - 15;
    edge.extend_bus_segment(1).unwrap();
    assert_eq!(edge.deadline, u64::MAX);
    assert_eq!(edge.instruction.cycles, 4);
    assert_eq!(edge.cpu.cycles(), 19);
    assert_eq!(edge.state, saved.state);
    assert_eq!(edge.memory, saved.memory);
    assert_eq!(edge.writes, saved.writes);
    high.extend_bus_segment(2).unwrap();
    assert_eq!(high.deadline, 150);
    let mut copied = machine.clone();
    for candidate in [&mut machine, &mut copied] {
        advance(candidate, 73);
        assert_eq!(candidate.bus.now.get(), 149);
        assert_eq!(&candidate.bus.ram[0x8000..0x8002], &[0xde, 0xde]);
        assert_eq!(candidate.sound_cpu.cycles(), 15);
        advance(candidate, 1);
        assert_eq!(&candidate.bus.ram[0x8000..0x8002], &[0xde, 0x12]);
        assert_eq!(candidate.sound_cpu.cycles(), 20);
        assert!(candidate.sound_schedule.pending.is_none());
        advance(candidate, 1);
        assert_eq!(candidate.sound_deadline(), Some(MasterTicks::new(195)));
        advance(candidate, 43);
        assert_eq!(&candidate.bus.ram[0x8000..0x8002], &[0xde, 0x12]);
        advance(candidate, 1);
        assert_eq!(&candidate.bus.ram[0x8000..0x8002], &[0x34, 0x12]);
        assert_eq!(candidate.sound_cpu.cycles(), 23);
    }
    assert_eq!(machine, copied);
    let mut partial = fixture(0x2080);
    advance(&mut partial, 64);
    assert_eq!(partial.bus.now.get(), 120);
    assert_eq!(&partial.bus.ram[0x8000..0x8002], &[0xde, 0x12]);
    advance(&mut partial, 1);
    let low = partial.sound_schedule.pending.as_mut().unwrap();
    let before = low.clone();
    assert!(low.extend_bus_segment(u64::MAX).is_err());
    assert_eq!(*low, before);
    partial.reset_sound_cpu();
    advance(&mut partial, 100);
    assert_eq!(&partial.bus.ram[0x8000..0x8002], &[0xde, 0x12]);

    let mut memory = partial.bus.sound.clone();
    memory.ram[0] = 0;
    let mut input = CandidateBus::new(&mut memory, &partial.bus.cartridge);
    let mut cpu = Cpu::new_with_reset(ResetState::new(0, 0x8002));
    let mut state = zeff_z80::BusState::default();
    let mut report = cpu.step_bus_segment(&mut input, &mut state).unwrap();
    assert!(report.complete);
    assert!(!state.pending());
    let mut encoded = zeff_emu_common::save_state::StateWriter::new();
    cpu.write_state(&mut encoded);
    let mut bytes = encoded.into_bytes();
    let mut shifted = cpu.clone();
    assert!(shifted.extend_bus_segment(&mut report.instruction, 1));
    let mut comparison = zeff_emu_common::save_state::StateWriter::new();
    shifted.write_state(&mut comparison);
    let comparison = comparison.into_bytes();
    let differences: Vec<_> = bytes
        .iter()
        .zip(&comparison)
        .enumerate()
        .filter_map(|(index, (a, b))| (a != b).then_some(index))
        .collect();
    assert_eq!(differences.len(), 1);
    let offset = differences[0];
    assert_eq!(
        u64::from_le_bytes(bytes[offset..offset + size_of::<u64>()].try_into().unwrap()),
        4
    );
    bytes[offset..offset + size_of::<u64>()].copy_from_slice(&(u64::MAX - 2).to_le_bytes());
    let mut reader = zeff_emu_common::save_state::StateReader::new(&bytes);
    cpu.read_state(&mut reader).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(cpu.cycles(), u64::MAX - 2);
    partial.sound_cpu = cpu;
    partial.bus.sound.ram[1] = 0xe5;
    let before = partial.clone();
    assert_eq!(
        crate::hardware::sound::schedule::PendingSound::prepare(
            &partial.sound_cpu,
            &partial.bus.sound,
            &partial.bus.cartridge,
            partial.bus.now.get()
        ),
        Err(crate::hardware::sound::SoundFault::ClockOverflow)
    );
    assert_eq!(partial, before);
    assert_eq!(&partial.bus.ram[0x8000..0x8002], &[0xde, 0x12]);
}
