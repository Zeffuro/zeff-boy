use super::*;
use crate::hardware::vdp::{VdpInterrupts, VdpRegisterPipeline};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::{ClockedBus, InterruptAperture, ObservedCycle};

fn fixture(concurrent: bool) -> Emulator {
    let mut image = vec![0; 0x400];
    image[..8].copy_from_slice(&[0, 0xff, 0x10, 0, 0, 0, 1, 0]);
    for level in 1..=7 {
        image[(24 + level) * 4..(24 + level) * 4 + 4].copy_from_slice(&[0, 0, 2, 0]);
    }
    for (index, word) in [
        0x33fc_u16, 0x8164, 0xc0, 4, 0x33fc, 0x8104, 0xc0, 4, 0x4ef9, 0, 0x200,
    ]
    .into_iter()
    .enumerate()
    {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    for address in (0x200..0x220).step_by(2) {
        image[address..address + 2].copy_from_slice(&0x4e71_u16.to_be_bytes());
    }
    let mut machine = Emulator::new(
        image,
        LoadConfig {
            concurrent_sound: concurrent,
            ..LoadConfig::default()
        },
    )
    .unwrap();
    let mut registers = VdpRegisterPipeline::at_h32_edge(MasterTicks::ZERO);
    for (index, value) in [(0, 4), (1, 4), (12, 0), (10, 255)] {
        registers
            .advance_to(MasterTicks::new(registers.now().get() + 1))
            .unwrap();
        let write = registers.queue_register(index, value).unwrap();
        registers.advance_to(write.publish_at).unwrap();
    }
    let now = registers.now();
    let interrupts = VdpInterrupts::at_counters_zero(now, 255, false, true);
    machine.bus.vdp = Vdp::from_known_state(interrupts, registers, Region::NtscUs).unwrap();
    machine.bus.now = now;
    machine.bus.cpu_clock = CpuClocks(0);
    let mut registers = machine.registers();
    registers.pc = 0x100;
    registers.set_status_register(0x2000);
    machine.cpu = PrefetchCpu::from_prefetched(registers, [0x33fc, 0x8164], CpuClocks(0)).unwrap();
    machine
}

fn prefetch() -> BusAccess {
    BusAccess {
        logical_address: 0x100,
        bus_address: 0x100,
        function_code: FunctionCode::SupervisorProgram,
        kind: AccessKind::Prefetch,
        lanes: ByteLanes::Both,
    }
}

fn read(machine: &mut Emulator, observed: bool, start: CpuClocks) -> ObservedCycle<u16, BusError> {
    let call = |bus: &mut dyn ClockedBus<Error = BusError>| {
        if observed {
            bus.read_cycle_observed(prefetch(), start, InterruptAperture::FollowingPrefetch)
        } else {
            ObservedCycle::unobserved(bus.read_cycle(prefetch(), start))
        }
    };
    if machine.concurrent_sound {
        call(&mut ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        ))
    } else {
        call(&mut machine.bus)
    }
}

pub(super) fn run() {
    for concurrent in [false, true] {
        cycle_equivalence(&super::external_dma::fixture(concurrent, false), true);
        let mut machine = fixture(concurrent);
        assert_eq!(machine.interrupt_level(), 0);
        assert!(matches!(
            machine.step_instruction().unwrap().cpu,
            TimedOutcome::Instruction(_)
        ));
        assert_eq!(machine.registers().pc, 0x108);
        assert_eq!(machine.interrupt_level(), 6);
        for frontend in [false, true] {
            let encoded = if frontend {
                machine.encode_frontend_state().unwrap()
            } else {
                machine.encode_state().unwrap()
            };
            let mut restored = fixture(concurrent);
            if frontend {
                restored.decode_frontend_state(&encoded).unwrap();
            } else {
                restored.decode_state(&encoded).unwrap();
            }
            assert_eq!(machine.cpu, restored.cpu);
            let mut control = machine.clone();
            if frontend {
                control.decode_frontend_state(&encoded).unwrap();
            } else {
                control.decode_state(&encoded).unwrap();
            }
            assert!(matches!(
                control.step_instruction().unwrap().cpu,
                TimedOutcome::Instruction(_)
            ));
            assert!(matches!(
                restored.step_instruction().unwrap().cpu,
                TimedOutcome::Instruction(_)
            ));
            assert!(control == restored, "paired restore continuation diverged");
            assert_eq!(control.registers().pc, 0x110);
            assert_eq!(control.interrupt_level(), 0);
            assert!(matches!(
                control.step_instruction().unwrap().cpu,
                TimedOutcome::Interrupt {
                    level: 6,
                    vector: 30,
                    ..
                }
            ));
            assert!(matches!(
                restored.step_instruction().unwrap().cpu,
                TimedOutcome::Interrupt {
                    level: 6,
                    vector: 30,
                    ..
                }
            ));
            assert!(control == restored, "paired restore continuation diverged");
            assert_eq!(control.registers().pc, 0x200);
            assert_eq!(control.peek(0xff0ffe), Some(1));
            assert_eq!(control.peek(0xff0fff), Some(0x10));
        }
        for level in [0, 4, 7] {
            let mut actual = machine.clone();
            actual.bus.irq_level = level;
            let mut control = actual.clone();
            let start = actual.bus.cpu_clock;
            let result = read(&mut actual, true, start);
            let plain = read(&mut control, false, start);
            assert_eq!(result.cycle, plain.cycle);
            assert!(actual == control, "observed callback changed machine");
            if level == 7 {
                assert!(result.samples.is_none());
            } else {
                assert_eq!(
                    result.samples.unwrap().levels(),
                    if level == 4 { (4, 4) } else { (6, 6) }
                );
            }
        }
        let mut edge = super::vdp::ready(1000 + 224 * 3420, concurrent);
        let mut plain_edge = edge.clone();
        let start = edge.bus.cpu_clock;
        let observed = read(&mut edge, true, start);
        let plain = read(&mut plain_edge, false, start);
        assert_eq!(observed.cycle, plain.cycle);
        assert_eq!(observed.samples.unwrap().levels(), (4, 6));
        assert_eq!(edge.interrupt_level(), 6);
        assert!(
            edge == plain_edge,
            "projecting the VDP edge changed machine"
        );
        let mut actual = fixture(concurrent);
        let mut control = actual.clone();
        let wrong = CpuClocks(actual.bus.cpu_clock.0 + 1);
        let observed = read(&mut actual, true, wrong);
        let plain = read(&mut control, false, wrong);
        assert_eq!(observed.cycle, plain.cycle);
        assert!(observed.samples.is_none());
        assert!(actual == control, "observed callback changed machine");
        let mut original = super::machine(&[0x4e71; 4]);
        original.concurrent_sound = concurrent;
        let mut control = original.clone();
        let start = original.bus.cpu_clock;
        let observed = read(&mut original, true, start);
        let plain = read(&mut control, false, start);
        assert!(observed.samples.is_none());
        assert_eq!(observed.cycle, plain.cycle);
        assert!(original == control, "port-history fallback changed machine");
    }
}

pub(super) fn cycle_equivalence(machine: &Emulator, held: bool) {
    let mut observed = machine.clone();
    let mut control = machine.clone();
    let start = observed.bus.cpu_clock;
    let cycle = read(&mut observed, true, start);
    let plain = read(&mut control, false, start);
    assert_eq!(cycle.cycle, plain.cycle);
    if held {
        assert!(cycle.cycle.wait_clocks > 0);
    }
    if cycle.cycle.wait_clocks != 0 {
        assert!(cycle.samples.is_none());
    }
    assert!(observed == control, "held-cycle fallback changed machine");
}
