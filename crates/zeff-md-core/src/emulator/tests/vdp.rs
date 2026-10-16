use super::*;
use crate::hardware::vdp::{VdpInterrupts, VdpRegisterPipeline};

const EPOCH: u64 = 1000;
const VINT: u64 = EPOCH + 766090;
const ENTRY_CLOCKS: u64 = 44;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

pub(super) fn ready(at: u64, concurrent: bool) -> Emulator {
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(EPOCH - 80));
    for (index, value) in [(0, 0x10), (1, 0x24), (12, 0), (10, 255)] {
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let write = registers.queue_register(index, value).unwrap();
        registers.advance_to(write.publish_at).unwrap();
    }
    let interrupts = VdpInterrupts::at_counters_zero(tick(EPOCH), 255, true, false);
    let mut vdp = Vdp::from_known_state(interrupts, registers, Region::NtscUs).unwrap();
    vdp.advance_to(tick(at), Region::NtscUs).unwrap();
    let mut image = vec![0; 0x400];
    image[..8].copy_from_slice(&[0, 0xff, 0x10, 0, 0, 0, 1, 0]);
    for level in 1..=7 {
        let index = (24 + level) * 4;
        image[index..index + 4].copy_from_slice(&[0, 0, 2, 0]);
    }
    image[0x100..0x104].copy_from_slice(&[0x4e, 0x71, 0x4e, 0x71]);
    image[0x200..0x204].copy_from_slice(&[0x4e, 0x71, 0x72, 1]);
    let mut machine = Emulator::new(
        image,
        LoadConfig {
            concurrent_sound: concurrent,
            ..LoadConfig::default()
        },
    )
    .unwrap();
    let mut registers = machine.registers();
    registers.set_status_register(0x2000);
    machine.cpu = PrefetchCpu::from_prefetched(registers, [0x4e71; 2], CpuClocks(0)).unwrap();
    machine.bus.cpu_clock = CpuClocks(0);
    machine.bus.now = tick(at);
    machine.bus.vdp = vdp;
    machine
}

pub(super) fn entry(machine: &Emulator, step: &MachineStep, level: u8, clocks: u64) {
    assert_eq!(
        step.cpu,
        TimedOutcome::Interrupt {
            level,
            vector: 24 + level,
            clocks
        }
    );
    assert_eq!(step.elapsed.get(), clocks * MAIN_CPU_DIVISOR);
    assert_eq!(machine.registers().pc, 0x200);
    assert_eq!(machine.registers().ssp, 0xff0ffa);
    assert_eq!(
        machine.registers().status_register(),
        0x2000 | (u16::from(level) << 8)
    );
    assert_eq!(machine.queued_words(), Some([0x4e71, 0x7201]));
    assert_eq!(machine.peek(0xff0ffa), Some(0x20));
    assert_eq!(machine.peek(0xff0ffb), Some(0));
    assert_eq!(machine.peek(0xff0ffe), Some(1));
    assert_eq!(machine.peek(0xff0fff), Some(0));
    assert_eq!(machine.bus.now, machine.vdp().now());
}

pub fn current_request_at_acknowledge_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        for (start, clears_vertical) in [(VINT - 80, true), (VINT - 120, false)] {
            let mut machine = ready(start, concurrent);
            assert_eq!(machine.interrupt_level(), 4);
            let outcome = step(&mut machine).unwrap();
            entry(&machine, &outcome, 4, ENTRY_CLOCKS);
            assert_eq!(
                machine.vdp().interrupts().horizontal_pending(),
                clears_vertical
            );
            assert_eq!(
                machine.vdp().interrupts().vertical_pending(),
                !clears_vertical
            );
        }
    }
}

#[test]
fn current_request_at_acknowledge() {
    current_request_at_acknowledge_with(Emulator::step_instruction);
}

pub fn diagnostic_override_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        for level in 1..=7 {
            let mut machine = ready(VINT + 1, concurrent);
            assert_eq!(machine.interrupt_level(), 6);
            assert!(machine.set_interrupt_level(level));
            assert!(!machine.set_interrupt_level(8));
            let outcome = step(&mut machine).unwrap();
            entry(&machine, &outcome, level, ENTRY_CLOCKS);
            assert!(machine.vdp().interrupts().horizontal_pending());
            assert!(machine.vdp().interrupts().vertical_pending());
            machine.set_interrupt_level(0);
            assert_eq!(machine.interrupt_level(), 6);
            machine.cpu.registers.set_status_register(0x2000);
            assert!(matches!(
                step(&mut machine).unwrap().cpu,
                TimedOutcome::Interrupt {
                    level: 6,
                    vector: 30,
                    ..
                }
            ));
            assert!(!machine.vdp().interrupts().vertical_pending());
            assert!(machine.vdp().interrupts().horizontal_pending());
        }
    }
}

#[test]
fn diagnostic_override() {
    diagnostic_override_with(Emulator::step_instruction);
}

pub fn acknowledge_collision_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        let mut machine = ready(VINT - 98, concurrent);
        assert_eq!(machine.interrupt_level(), 4);
        let outcome = step(&mut machine).unwrap();
        entry(&machine, &outcome, 4, ENTRY_CLOCKS);
        assert_eq!(
            machine.bus.now,
            tick(VINT - 98 + ENTRY_CLOCKS * MAIN_CPU_DIVISOR)
        );
        assert!(machine.vdp().interrupts().horizontal_pending());
        assert!(!machine.vdp().interrupts().vertical_pending());
        assert_eq!(machine.interrupt_level(), 4);
        assert!(step(&mut machine).is_ok());
    }
}

#[test]
fn acknowledge_collision() {
    acknowledge_collision_with(Emulator::step_instruction);
}

pub fn reset_domains() {
    for concurrent in [false, true] {
        let mut machine = ready(VINT + 1, concurrent);
        let before = machine.vdp().clone();
        machine.reset_cpu().unwrap();
        let mut expected = before;
        expected
            .advance_to(
                tick(VINT + 1 + PRIME_CLOCKS * MAIN_CPU_DIVISOR),
                Region::NtscUs,
            )
            .unwrap();
        assert_eq!(machine.vdp(), &expected);
        machine.cpu = PrefetchCpu::from_prefetched(
            machine.cpu.registers,
            [0x4e70, 0x4e71],
            machine.cpu_clock(),
        )
        .unwrap();
        machine.set_interrupt_level(1);
        machine.step_instruction().unwrap();
        assert_eq!(machine.reset_pulses(), 1);
        assert!(machine.vdp().interrupts().vertical_pending());
        assert_eq!(machine.vdp().registers().registers()[1], Some(0x24));
        machine.bus.now = tick(u64::MAX - 1);
        let saved = machine.clone();
        assert_eq!(machine.reset_cpu(), Err(TimingFault::ClockOverflow));
        assert_eq!(machine, saved);
        Reset::reset(&mut machine);
        let mut bootstrap = Vdp::bootstrap_mode5();
        bootstrap
            .advance_to(tick(PRIME_CLOCKS * MAIN_CPU_DIVISOR), Region::NtscUs)
            .unwrap();
        assert_eq!(machine.vdp(), &bootstrap);
        assert_eq!(machine.interrupt_level(), 0);
    }
}

#[test]
fn reset_domains_case() {
    reset_domains();
}

pub fn stop_and_sound_gate() {
    let mut machine = ready(VINT - 30, false);
    machine.cpu.registers.set_status_register(0x2700);
    machine.cpu =
        PrefetchCpu::from_prefetched(machine.cpu.registers, [0x4e72, 0x2700], CpuClocks(0))
            .unwrap();
    machine.step_instruction().unwrap();
    assert_eq!(
        machine.step_instruction().unwrap().cpu,
        TimedOutcome::Stopped
    );
    assert_eq!(machine.interrupt_level(), 6);
    assert_eq!(
        machine.step_sound_instruction(),
        Err(SoundFault::MainInterruptPending)
    );
    machine.cpu.registers.set_status_register(0x2000);
    assert!(matches!(
        machine.step_instruction().unwrap().cpu,
        TimedOutcome::Interrupt { level: 6, .. }
    ));
}

#[test]
fn stop_and_sound_gate_case() {
    stop_and_sound_gate();
}

pub fn reservation_acknowledge() {
    use crate::hardware::sound::{BankAccess, CandidateBus, schedule::PendingSound};
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
    use zeff_m68k::timing::ClockedBus;

    for (start, release) in [(VINT - 35, VINT), (VINT - 56, VINT - 28)] {
        let mut machine = ready(start, true);
        machine.bus.sound.set_reset(false);
        machine.bus.sound.bank = 0x1ff;
        machine.bus.sound.ram[..5].copy_from_slice(&[0x3e, 0x5a, 0x32, 0, 0x80]);
        let mut snapshot = machine.bus.sound.clone();
        let mut setup = CandidateBus::new(&mut snapshot, &machine.bus.cartridge);
        machine.sound_cpu.step_with_bus(&mut setup).unwrap();
        let mut pending = PendingSound::prepare(
            &machine.sound_cpu,
            &machine.bus.sound,
            &machine.bus.cartridge,
            start,
        )
        .unwrap();
        pending.start = start;
        pending.deadline = release;
        pending.bank_access = Some(BankAccess {
            address: 0xff8000,
            direction: Direction::Write,
            value: 0x5a,
        });
        machine.bus.sound.requested = true;
        machine.sound_schedule.pending = Some(pending);
        let response = ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .acknowledge_cycle(
            BusAccess {
                logical_address: 0xfffffff8,
                bus_address: 0xfffff8,
                function_code: FunctionCode::CpuSpace,
                kind: AccessKind::InterruptAcknowledge,
                lanes: ByteLanes::Both,
            },
            CpuClocks(0),
        );
        assert_eq!(
            response.wait_clocks,
            ((release - start) / MAIN_CPU_DIVISOR) as u32
        );
        assert_eq!(machine.peek(0xff8000), Some(0x5a));
        assert!(machine.sound_schedule.pending.is_none());
        assert!(machine.sound_bus_granted());
        assert!(machine.vdp().interrupts().horizontal_pending());
        assert!(!machine.vdp().interrupts().vertical_pending());
        assert_eq!(
            response.result,
            Ok(zeff_m68k::bus::BusResponse::Complete(
                zeff_m68k::timing::InterruptVector::Autovector
            ))
        );
        assert_eq!(machine.bus.now, tick(release + 28));
        assert_eq!(machine.bus.cpu_clock.0, u64::from(response.wait_clocks) + 4);
        assert_eq!(machine.vdp().now(), machine.bus.now);
    }
}

#[test]
fn reservation_acknowledge_case() {
    reservation_acknowledge();
}
