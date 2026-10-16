use super::*;
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::hardware::sound::{BankAccess, CandidateBus};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CycleResult};
use zeff_z80::{Cpu, ResetState, Z80Bus};

struct TraceBus<'a> {
    inner: ScheduledBus<'a>,
    reads: Vec<(u32, u16, u64, u32)>,
}

impl ClockedBus for TraceBus<'_> {
    type Error = BusError;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        let result = self.inner.read_cycle(access, start);
        if let Ok(value) = result.result {
            self.reads.push((
                access.bus_address,
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
        self.inner.write_cycle(access, value, start)
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.inner.idle(start, clocks);
    }
}

fn ready(sound: &[u8]) -> Emulator {
    let mut machine = super::machine(&[0x4e71, 0x2010, 0x4e71, 0x4e71]);
    machine.bus.ram.fill(0xde);
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x1ff;
    machine.bus.sound.ram[..3].copy_from_slice(&[0x21, 0, 0x80]);
    machine.bus.sound.ram[3..3 + sound.len()].copy_from_slice(sound);
    machine.sound_cpu = Cpu::new_with_reset(ResetState::new(0, 0x8002));
    let mut snapshot = machine.bus.sound.clone();
    let mut setup = CandidateBus::new(&mut snapshot, &machine.bus.cartridge);
    machine.sound_cpu.step_with_bus(&mut setup).unwrap();
    machine.bus.now = MasterTicks::ZERO;
    machine.bus.audio = crate::hardware::audio::Audio::default();
    machine.bus.vdp = Vdp::bootstrap_mode5();
    machine.bus.cpu_clock = CpuClocks(0);
    machine.cpu = PrefetchCpu::default();
    machine.cpu.registers = machine.bus.cartridge.initial;
    machine.cpu.registers.a[0] = 0xff7ffe;
    machine.concurrent_sound = true;
    machine
        .cpu
        .prime(&mut ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        })
        .unwrap();
    machine.step_instruction().unwrap();
    assert_eq!(machine.bus.now.get(), 84);
    machine
}

#[test]
fn automatic_load_reservation_samples_after_one_finite_hold() {
    let mut machine = ready(&[0x36, 0x55, 0x76]);
    let mut copied = machine.clone();
    for candidate in [&mut machine, &mut copied] {
        let mut bus = TraceBus {
            inner: ScheduledBus {
                bus: &mut candidate.bus,
                cpu: &mut candidate.sound_cpu,
                schedule: &mut candidate.sound_schedule,
            },
            reads: Vec::new(),
        };
        let step = candidate.cpu.step(&mut bus).unwrap();
        assert_eq!(step.clocks, 20);
        assert_eq!(
            bus.reads,
            [
                (0xff7ffe, 0xdede, 112, 0),
                (0xff8000, 0x55de, 196, 8),
                (0x106, 0x4e71, 224, 0)
            ]
        );
        assert_eq!(&bus.inner.bus.ram[0x8000..0x8002], &[0x55, 0xde]);
        assert_eq!(candidate.cpu.registers.d[0], 0xdede55de);
        assert_eq!(candidate.cpu.clock(), CpuClocks(32));
        assert_eq!(candidate.sound_cpu.cycles(), 21);
    }
    assert_eq!(machine, copied);
}

fn read(address: u32) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address,
        kind: AccessKind::Data,
        lanes: ByteLanes::Both,
        function_code: FunctionCode::SupervisorData,
    }
}

#[test]
fn bank_access_is_latched_and_waiting_main_gets_the_next_turn() {
    let mut machine = ready(&[0x22, 0, 0x80, 0x76]);
    let mut bus = ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    };
    bus.idle(CpuClocks(12), 14);
    assert_eq!(bus.bus.now.get(), 182);
    let low = bus.schedule.pending.as_ref().unwrap();
    assert_eq!(low.start, 150);
    assert_eq!(low.deadline, 195);
    assert_eq!(
        low.bank_access,
        Some(BankAccess {
            address: 0xff8000,
            direction: Direction::Write,
            value: 0
        })
    );
    bus.bus.sound.bank = 0x1c0;
    let result = bus.read_cycle(read(0xff8000), CpuClocks(26));
    assert_eq!(result.wait_clocks, 2);
    assert_eq!(result.result, Ok(0x00de));
    assert_eq!(bus.bus.now.get(), 224);
    assert_eq!(bus.bus.ram[0x8000], 0);
    assert_eq!(bus.bus.ram[0x8001], 0xde);
    assert_eq!(bus.bus.ram[0], 0xde);
    let high = bus.schedule.pending.as_ref().unwrap();
    assert_eq!(high.start, 225);
    assert_eq!(high.deadline, 270);
    assert_eq!(
        high.bank_access,
        Some(BankAccess {
            address: 0xff8001,
            direction: Direction::Write,
            value: 0x80
        })
    );
    let result = bus.read_cycle(read(0xff8000), CpuClocks(32));
    assert_eq!(result.wait_clocks, 7);
    assert_eq!(result.result, Ok(0x0080));
    assert_eq!(bus.bus.now.get(), 301);
    assert_eq!(bus.bus.ram[0x8001], 0x80);
    assert_eq!(bus.bus.ram[1], 0xde);
}

#[test]
fn automatic_mapped_fault_retains_waits_and_prior_sound_write() {
    let mut machine = ready(&[0x36, 0x55, 0x76]);
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
    assert_eq!(machine.bus.now.get(), 196);
    assert_eq!(machine.cpu.clock(), CpuClocks(28));
    assert_eq!(machine.bus.cpu_clock, CpuClocks(28));
    assert_eq!(machine.bus.ram[0x8000], 0x55);
    assert_eq!(machine.cpu.registers.d[0], 0);
    let saved = machine.clone();
    assert_eq!(
        machine.step_instruction(),
        Err(MachineFault::Main(TimingFault::Frozen))
    );
    assert_eq!(machine, saved);
}

#[test]
fn concurrent_reset_rolls_back_first_and_second_prime_failures() {
    for (headroom, release) in [(56, 15), (100, 50)] {
        let mut machine = ready(&[0x36, 0x55, 0x76]);
        ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .idle(CpuClocks(12), 4);
        let pending = machine.sound_schedule.pending.as_mut().unwrap();
        assert_eq!(pending.bank_access.unwrap().value, 0x55);
        pending.start = u64::MAX - release - 45;
        pending.deadline = u64::MAX - release;
        machine.bus.now = MasterTicks::new(u64::MAX - headroom);
        let saved = machine.clone();
        assert!(machine.reset_cpu().is_err());
        assert_eq!(machine, saved);
        assert_eq!(machine.bus.ram[0x8000], 0xde);
    }
}

#[test]
fn reservation_overflow_discards_only_unpublished_phase() {
    let mut machine = ready(&[0x36, 0x55, 0x76]);
    ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .idle(CpuClocks(12), 4);
    machine.bus.ram[0x8004] = 0xa6;
    let cpu = machine.sound_cpu.clone();
    let memory = machine.bus.sound.clone();
    machine.sound_schedule.advance(
        &mut machine.sound_cpu,
        &mut machine.bus.sound,
        &machine.bus.cartridge,
        &mut machine.bus.ram,
        crate::hardware::sound::schedule::SoundInterval {
            sound_irq: None,
            audio: &mut machine.bus.audio,
            ticks: 112..113,
            occupied: &[Reservation {
                requester: Requester::MAIN,
                ticks: 112..u64::MAX,
            }],
        },
    );
    assert_eq!(machine.sound_fault(), Some(SoundFault::ClockOverflow));
    assert!(machine.sound_schedule.pending.is_none());
    assert_eq!(machine.sound_cpu, cpu);
    assert_eq!(machine.bus.sound, memory);
    assert_eq!(machine.bus.ram[0x8000], 0xde);
    assert_eq!(machine.bus.ram[0x8004], 0xa6);
}

#[test]
fn unclassified_and_multiple_accesses_never_gain_phase_ownership() {
    let machine = ready(&[0x36, 0x55]);
    let mut snapshot = machine.bus.sound.clone();
    let mut bus = CandidateBus::new(&mut snapshot, &machine.bus.cartridge);
    assert_eq!(bus.cpu_read(0x8000), 0xff);
    bus.io_write(0, 0);
    bus.cpu_write(0x8001, 0x55);
    assert_eq!(bus.accesses.get(), 3);
    assert_eq!(bus.bank_access.get().unwrap().address, 0xff8000);
    assert_eq!(bus.bank_access.get().unwrap().direction, Direction::Read);
    let pending = crate::hardware::sound::schedule::PendingSound::prepare(
        &machine.sound_cpu,
        &machine.bus.sound,
        &machine.bus.cartridge,
        machine.bus.now.get(),
    )
    .unwrap();
    assert!(pending.bank_access.is_none());
}
