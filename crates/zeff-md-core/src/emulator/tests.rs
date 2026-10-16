use super::*;

#[path = "tests/pal.rs"]
mod pal;

#[path = "tests/external_dma.rs"]
mod external_dma;

#[path = "tests/psg.rs"]
mod psg;

#[path = "tests/opn2.rs"]
mod opn2;

#[path = "tests/vdp_register_order.rs"]
mod vdp_register_order;

#[path = "tests/bank_reservation.rs"]
mod bank_reservation;
#[path = "tests/banked_ram.rs"]
mod banked_ram;
#[path = "tests/open_bus.rs"]
mod open_bus;
#[path = "tests/requesters.rs"]
mod requesters;
#[path = "tests/sound_control.rs"]
mod sound_control;
#[path = "tests/vdp.rs"]
mod vdp;
#[path = "tests/vdp_capture.rs"]
mod vdp_capture;
#[path = "tests/vdp_data.rs"]
mod vdp_data;
#[path = "tests/vdp_dma.rs"]
mod vdp_dma;
#[path = "tests/vdp_dma_cpu.rs"]
mod vdp_dma_cpu;
#[path = "tests/vdp_dma_irq.rs"]
mod vdp_dma_irq;
#[path = "tests/vdp_dma_long.rs"]
mod vdp_dma_long;
#[path = "tests/vdp_hv.rs"]
mod vdp_hv;
#[path = "tests/vdp_line.rs"]
mod vdp_line;
#[path = "tests/vdp_logical_line.rs"]
mod vdp_logical_line;
#[path = "tests/vdp_ports.rs"]
mod vdp_ports;
#[path = "tests/vdp_prefetch.rs"]
mod vdp_prefetch;
#[path = "tests/vdp_row_delivery.rs"]
mod vdp_row_delivery;
#[path = "tests/vdp_row_delivery_edges.rs"]
mod vdp_row_delivery_edges;
#[path = "tests/vdp_sat_live.rs"]
mod vdp_sat_live;
#[path = "tests/vdp_stable_display.rs"]
mod vdp_stable_display;
#[path = "tests/vdp_status_port.rs"]
mod vdp_status_port;
#[path = "tests/waits.rs"]
mod waits;

fn main_fault(fault: MachineFault) -> TimingFault<BusError> {
    match fault {
        MachineFault::Main(fault) => fault,
        MachineFault::Sound(fault) => panic!("sound fault: {fault:?}"),
    }
}

fn machine(program: &[u16]) -> Emulator {
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in program.iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(image, LoadConfig::default()).unwrap()
}

#[test]
fn transfer_overflow_cannot_deposit_ram_or_restart() {
    let mut machine = machine(&[0x7055, 0x3080, 0x4e71, 0x4e71]);
    machine.step_instruction().unwrap();
    machine.cpu.registers.a[0] = 0xff_0000;
    machine.bus.now = MasterTicks::new(u64::MAX - 1);
    assert!(matches!(
        machine.step_instruction().map_err(main_fault),
        Err(TimingFault::Bus {
            source: BusError::ClockOverflow,
            ..
        })
    ));
    assert_eq!(machine.peek(0xff_0000), Some(0));
    assert_eq!(machine.peek(0xff_0001), Some(0));
    assert_eq!(machine.bus.now.get(), u64::MAX - 1);
    let saved = machine.clone();
    assert_eq!(
        machine.step_instruction().map_err(main_fault),
        Err(TimingFault::Frozen)
    );
    assert_eq!(machine.reset_cpu(), Err(TimingFault::ClockOverflow));
    assert_eq!(machine, saved);
}

#[test]
fn multi_load_deadline_overflow_preserves_completed_write() {
    let mut machine = machine(&[0x4e71, 0x4e71]);
    machine.bus.sound.set_reset(false);
    machine.bus.sound.ram[..6].copy_from_slice(&[0x21, 0x34, 0x12, 0x22, 0, 1]);
    machine.sound_schedule.advance(
        &mut machine.sound_cpu,
        &mut machine.bus.sound,
        &machine.bus.cartridge,
        &mut machine.bus.ram,
        crate::hardware::sound::schedule::SoundInterval::new(0..345, &mut machine.bus.audio),
    );
    assert_eq!(machine.sound_cpu.cycles(), 23);
    assert_eq!(machine.peek(0xa0_0100), Some(0x34));
    assert_eq!(machine.peek(0xa0_0101), Some(0));
    let cpu = machine.sound_cpu.clone();
    assert!(machine.sound_schedule.pending.is_none());
    machine.bus.now = MasterTicks::new(u64::MAX - 28);
    machine.concurrent_sound = true;
    assert_eq!(
        machine.step_instruction(),
        Err(MachineFault::Sound(SoundFault::ClockOverflow))
    );
    assert_eq!(machine.sound_cpu, cpu);
    assert_eq!(machine.peek(0xa0_0100), Some(0x34));
    assert_eq!(machine.peek(0xa0_0101), Some(0));
    let saved = machine.clone();
    assert_eq!(
        machine.step_instruction(),
        Err(MachineFault::Sound(SoundFault::ClockOverflow))
    );
    assert_eq!(machine, saved);
}

#[test]
fn idle_overflow_is_stable_and_whole_reset_recovers() {
    let mut machine = machine(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
    machine.bus.now = MasterTicks::new(u64::MAX - 1);
    assert_eq!(
        machine.step_instruction().map_err(main_fault),
        Err(TimingFault::ClockOverflow)
    );
    assert!(machine.bus.clock_fault);
    assert_eq!(machine.bus.now.get(), u64::MAX - 1);
    let saved = machine.clone();
    assert_eq!(
        machine.step_instruction().map_err(main_fault),
        Err(TimingFault::ClockOverflow)
    );
    assert_eq!(machine.reset_cpu(), Err(TimingFault::ClockOverflow));
    assert_eq!(machine, saved);
    Reset::reset(&mut machine);
    assert!(!machine.bus.clock_fault);
    assert_eq!(machine.bus.now.get(), 56);
    machine.step_instruction().unwrap();
}

#[test]
fn sound_completion_phases_and_overflow_are_transactional() {
    for phase in 0..crate::constants::SOUND_CPU_DIVISOR {
        let mut machine = machine(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
        machine.step_instruction().unwrap();
        machine.step_instruction().unwrap();
        machine.bus.sound.set_reset(false);
        machine.bus.sound.ram[..5].copy_from_slice(&[0x3e, 0x55, 0x32, 0, 1]);
        machine.bus.now = MasterTicks::new(150 + phase);
        machine.bus.audio = crate::hardware::audio::Audio::default();
        machine.bus.audio.advance_to(machine.bus.now.get());
        let main_clock = machine.cpu_clock();
        machine.step_sound_instruction().unwrap();
        assert_eq!(machine.timing_snapshot().now().get(), 255);
        assert_eq!(machine.cpu_clock(), main_clock);
        assert_eq!(machine.sound_cpu().regs().a, 0x55);
        machine.bus.now = MasterTicks::new(u64::MAX - 1);
        let before = machine.clone();
        assert_eq!(
            machine.step_sound_instruction(),
            Err(crate::SoundFault::ClockOverflow)
        );
        assert_eq!(machine, before);
        assert_eq!(
            machine.peek(crate::constants::SOUND_APERTURE_START + 0x100),
            Some(0)
        );
    }
}

#[test]
fn concurrent_deadline_control_ties_and_overflow() {
    use crate::constants::{SOUND_BUS_REQUEST, SOUND_RESET};
    use crate::hardware::sound::schedule::PendingSound;
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
    use zeff_m68k::timing::ClockedBus;
    for address in [SOUND_BUS_REQUEST, SOUND_RESET] {
        let mut machine = machine(&[0x4e71, 0x4e71]);
        machine.bus.sound.set_reset(false);
        machine.bus.sound.ram[..6].copy_from_slice(&[0x3e, 0x5a, 0x32, 0, 1, 0x76]);
        PendingSound::prepare(
            &machine.sound_cpu,
            &machine.bus.sound,
            &machine.bus.cartridge,
            0,
        )
        .unwrap()
        .publish(
            &mut machine.sound_cpu,
            &mut machine.bus.sound,
            &mut machine.bus.ram,
            &mut machine.bus.audio,
        );
        let pending = PendingSound::prepare(
            &machine.sound_cpu,
            &machine.bus.sound,
            &machine.bus.cartridge,
            105,
        )
        .unwrap();
        machine.bus.now = MasterTicks::new(pending.deadline - 28);
        machine.sound_schedule.pending = Some(pending);
        let access = BusAccess {
            logical_address: address,
            bus_address: address,
            function_code: FunctionCode::SupervisorData,
            kind: AccessKind::Data,
            lanes: ByteLanes::Both,
        };
        let clock = machine.bus.cpu_clock;
        let result = ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .write_cycle(
            access,
            if address == SOUND_BUS_REQUEST {
                0x100
            } else {
                0
            },
            clock,
        );
        assert_eq!(result.result, Ok(()));
        assert_eq!(result.wait_clocks, 0);
        assert_eq!(
            machine.peek(crate::constants::SOUND_APERTURE_START + 0x100),
            Some(0x5a)
        );
        assert!(machine.sound_schedule.pending.is_none());
        if address == SOUND_BUS_REQUEST {
            assert!(machine.sound_bus_granted());
        } else {
            assert_eq!(machine.sound_cpu().cycles(), 0);
        }
        machine.bus.now = MasterTicks::new(u64::MAX - 1);
        let saved = machine.clone();
        assert_eq!(
            ScheduledBus {
                bus: &mut machine.bus,
                cpu: &mut machine.sound_cpu,
                schedule: &mut machine.sound_schedule
            }
            .advance_machine(7),
            Err(BusError::ClockOverflow)
        );
        assert_eq!(machine, saved);
    }
}

#[test]
fn prefixed_pending_reset_ties_and_overflow() {
    use crate::constants::{SOUND_APERTURE_START, SOUND_RESET};
    use crate::hardware::sound::schedule::PendingSound;
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
    use zeff_m68k::timing::ClockedBus;
    for distance in [28, 56] {
        let mut machine = machine(&[0x4e71, 0x4e71]);
        machine.bus.sound.set_reset(false);
        machine.bus.sound.ram[..7].copy_from_slice(&[0x21, 0x34, 0x12, 0xed, 0x63, 0, 1]);
        PendingSound::prepare(
            &machine.sound_cpu,
            &machine.bus.sound,
            &machine.bus.cartridge,
            0,
        )
        .unwrap()
        .publish(
            &mut machine.sound_cpu,
            &mut machine.bus.sound,
            &mut machine.bus.ram,
            &mut machine.bus.audio,
        );
        let pending = PendingSound::prepare(
            &machine.sound_cpu,
            &machine.bus.sound,
            &machine.bus.cartridge,
            150,
        )
        .unwrap();
        assert_eq!(pending.instruction.cycles, 20);
        assert_eq!(pending.deadline, 450);
        machine.bus.now = MasterTicks::new(pending.deadline - distance);
        machine.sound_schedule.pending = Some(pending);
        let saved = machine.clone();
        assert_eq!(
            PendingSound::prepare(
                &machine.sound_cpu,
                &machine.bus.sound,
                &machine.bus.cartridge,
                u64::MAX - 1
            ),
            Err(crate::SoundFault::ClockOverflow)
        );
        assert_eq!(machine, saved);
        let clock = machine.bus.cpu_clock;
        let access = BusAccess {
            logical_address: SOUND_RESET,
            bus_address: SOUND_RESET,
            function_code: FunctionCode::SupervisorData,
            kind: AccessKind::Data,
            lanes: ByteLanes::Both,
        };
        let result = ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .write_cycle(access, 0, clock);
        assert_eq!(result.result, Ok(()));
        let expected = if distance == 28 { [0x34, 0x12] } else { [0, 0] };
        assert_eq!(
            [
                machine.peek(SOUND_APERTURE_START + 0x100).unwrap(),
                machine.peek(SOUND_APERTURE_START + 0x101).unwrap()
            ],
            expected
        );
        assert!(machine.sound_schedule.pending.is_none());
        assert_eq!(machine.sound_cpu().cycles(), 0);
    }
}

#[test]
fn indexed_prefix_reset_and_deadline_overflow() {
    use crate::constants::SOUND_RESET;
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
    use zeff_m68k::timing::ClockedBus;
    let mut machine = machine(&[0x4e71, 0x4e71]);
    machine.bus.sound.set_reset(false);
    machine.bus.sound.ram[..2].copy_from_slice(&[0xdd, 0xfd]);
    machine.set_interrupt_level(4);
    ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .advance_machine(7)
    .unwrap();
    assert_eq!(machine.sound_cpu().regs().pc, 0);
    assert!(machine.sound_deadline().is_some());
    machine.reset_sound_cpu();
    assert!(!machine.sound_prefix_pending());
    assert_eq!(machine.timing_snapshot().now().get(), 63);
    ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .advance_machine(60)
    .unwrap();
    assert_eq!(machine.sound_cpu().regs().pc, 1);
    assert!(machine.sound_prefix_pending());
    let clock = machine.bus.cpu_clock;
    let access = BusAccess {
        logical_address: SOUND_RESET,
        bus_address: SOUND_RESET,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes: ByteLanes::Both,
    };
    assert_eq!(
        ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule
        }
        .write_cycle(access, 0, clock)
        .result,
        Ok(())
    );
    assert_eq!(machine.sound_cpu().regs().pc, 0);
    assert!(!machine.sound_prefix_pending());
    assert!(machine.sound_deadline().is_none());
    assert_eq!(machine.interrupt_level(), 4);
    machine.bus.sound.set_reset(false);
    machine.bus.now = MasterTicks::new(u64::MAX - 28);
    let sound = machine.sound_cpu.clone();
    ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .advance_machine(7)
    .unwrap();
    assert_eq!(machine.sound_cpu(), &sound);
    assert_eq!(
        machine.sound_fault(),
        Some(crate::SoundFault::ClockOverflow)
    );
    assert!(!machine.sound_prefix_pending());
    assert!(machine.sound_deadline().is_none());
}

#[path = "tests/external_dma_sound.rs"]
mod external_dma_sound;
