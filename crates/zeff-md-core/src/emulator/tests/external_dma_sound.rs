use super::*;
use crate::constants::{MAIN_CPU_DIVISOR, SOUND_CPU_DIVISOR, TRANSFER_CLOCKS};
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::hardware::sound::CandidateBus;
use crate::hardware::sound::schedule::SoundInterval;
use crate::hardware::vdp::VdpDmaSource;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;
use zeff_z80::{Cpu, ResetState};

const T: u64 = SOUND_CPU_DIVISOR;

fn ready(program: &[u8], bank: u16) -> Emulator {
    let mut machine = machine(&[0x4e71, 0x4e71]);
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = bank;
    machine.bus.sound.ram[..program.len()].copy_from_slice(program);
    machine.bus.ram.fill(0xde);
    machine.bus.audio = crate::hardware::audio::Audio::default();
    machine
}

fn advance(machine: &mut Emulator, start: u64, end: u64, occupied: &[Reservation]) {
    machine.sound_schedule.advance(
        &mut machine.sound_cpu,
        &mut machine.bus.sound,
        &machine.bus.cartridge,
        &mut machine.bus.ram,
        SoundInterval {
            sound_irq: None,
            ticks: start..end,
            occupied,
            audio: &mut machine.bus.audio,
        },
    );
}

fn vdp(start: u64, end: u64) -> Reservation {
    Reservation {
        requester: Requester::VDP,
        ticks: start..end,
    }
}

fn ldi_pending(destination: u16, bank: u16, value: u8) -> Emulator {
    let [low, high] = destination.to_le_bytes();
    let mut machine = ready(
        &[0x21, 0, 1, 0x11, low, high, 0x01, 1, 0, 0xed, 0xa0, 0x76],
        bank,
    );
    machine.bus.sound.ram[0x100] = value;
    let mut snapshot = machine.bus.sound.clone();
    let mut setup = CandidateBus::new(&mut snapshot, &machine.bus.cartridge);
    for _ in 0..3 {
        machine.sound_cpu.step_with_bus(&mut setup).unwrap();
    }
    assert_eq!(setup.fault.get(), None);
    assert_eq!(machine.sound_cpu.cycles(), 30);
    advance(&mut machine, 0, 11 * T + 1, &[]);
    let pending = machine.sound_schedule.pending.as_ref().unwrap();
    assert_eq!((pending.start, pending.deadline), (165, 210));
    assert_eq!(pending.instruction.cycles, 3);
    assert!(pending.any_bank_access);
    assert!(pending.bank_access.is_none());
    assert!(pending.reservation().is_none());
    assert_eq!(machine.sound_cpu.cycles(), 41);
    machine
}

#[test]
fn ldi_bank_write_waits_for_vdp_and_publishes_once_after_clone() {
    let mut machine = ldi_pending(0x8000, 0x1ff, 0x55);
    let cpu = machine.sound_cpu.clone();
    let memory = machine.bus.sound.clone();
    let state = machine.sound_schedule.state;
    let old_pending = machine.sound_schedule.pending.as_ref().unwrap().clone();
    let occupied = [vdp(10 * T, 40 * T)];
    advance(&mut machine, 11 * T + 1, 43 * T - 1, &occupied);
    assert_eq!(machine.sound_cpu, cpu);
    assert_eq!(machine.bus.sound, memory);
    assert_eq!(machine.sound_schedule.state, state);
    assert_eq!(machine.bus.ram[0x8000], 0xde);
    let pending = machine.sound_schedule.pending.as_mut().unwrap();
    assert_eq!((pending.start, pending.deadline), (600, 645));
    assert_eq!(pending.instruction.cycles, 32);
    assert_eq!(pending.cpu.cycles(), old_pending.cpu.cycles() + 29);
    assert_eq!(pending.state, old_pending.state);
    let held = pending.clone();
    pending.reserve_after(&occupied).unwrap();
    assert_eq!(*pending, held);
    let mut copied = machine.clone();
    for candidate in [&mut machine, &mut copied] {
        advance(candidate, 43 * T - 1, 43 * T, &occupied);
        assert_eq!(candidate.bus.ram[0x8000], 0x55);
        assert_eq!(candidate.sound_cpu.cycles(), 73);
        assert_eq!(candidate.sound_schedule.state, held.state);
        assert!(candidate.sound_schedule.pending.is_none());
        candidate.bus.ram[0x8000] = 0xa6;
        advance(candidate, 43 * T, 45 * T, &occupied);
        assert_eq!(candidate.bus.ram[0x8000], 0xa6);
        assert_eq!(candidate.sound_cpu.regs().de(), 0x8001);
        assert_eq!(candidate.sound_cpu.regs().hl(), 0x101);
        assert_eq!(candidate.sound_cpu.regs().bc(), 0);
    }
    assert_eq!(machine, copied);
}

#[test]
fn unqualified_bank_hold_ignores_other_owners_and_clears_all_vdp_ranges() {
    let machine = ldi_pending(0x8000, 0x1ff, 0x55);
    let original = machine.sound_schedule.pending.as_ref().unwrap();
    let mut pending = original.clone();
    pending
        .reserve_after(&[
            Reservation {
                requester: Requester::MAIN,
                ticks: 0..u64::MAX,
            },
            Reservation {
                requester: Requester(9),
                ticks: 0..u64::MAX,
            },
            vdp(0, 11 * T),
            vdp(14 * T, 20 * T),
        ])
        .unwrap();
    assert_eq!(&pending, original);
    for occupied in [
        [vdp(40 * T, 44 * T), vdp(10 * T, 40 * T)],
        [vdp(10 * T, 40 * T), vdp(40 * T, 44 * T)],
    ] {
        let mut pending = original.clone();
        pending.reserve_after(&occupied).unwrap();
        assert_eq!((pending.start, pending.deadline), (44 * T, 47 * T));
        assert_eq!(pending.instruction.cycles, 36);
        let saved = pending.clone();
        pending.reserve_after(&occupied).unwrap();
        assert_eq!(pending, saved);
    }
    for phase in 1..T {
        let mut pending = original.clone();
        pending
            .reserve_after(&[vdp(10 * T, 40 * T + phase)])
            .unwrap();
        assert_eq!((pending.start, pending.deadline), (41 * T, 44 * T));
        assert_eq!(pending.instruction.cycles, 33);
    }
}

#[test]
fn banked_m1_rom_fetch_waits_for_vdp_without_qualified_ownership() {
    let mut machine = ready(&[], 0);
    machine.sound_cpu = Cpu::new_with_reset(ResetState::new(0x8000, 0xffff));
    let cpu = machine.sound_cpu.clone();
    let occupied = [vdp(0, 40 * T)];
    advance(&mut machine, 0, 44 * T - 1, &occupied);
    assert_eq!(machine.sound_cpu, cpu);
    let pending = machine.sound_schedule.pending.as_ref().unwrap();
    assert!(pending.any_bank_access);
    assert!(pending.reservation().is_none());
    assert_eq!((pending.start, pending.deadline), (40 * T, 44 * T));
    assert_eq!(pending.instruction.cycles, 44);
    advance(&mut machine, 44 * T - 1, 44 * T, &occupied);
    assert_eq!(machine.sound_cpu.regs().pc, 0x8001);
    assert_eq!(machine.sound_cpu.cycles(), 44);
    assert!(machine.sound_schedule.pending.is_none());
}

#[test]
fn unqualified_banked_psg_write_waits_but_local_sound_work_continues() {
    let occupied = [vdp(10 * T, 40 * T)];
    let mut banked = ldi_pending(0x8011, 0x180, 0x90);
    advance(&mut banked, 11 * T + 1, 43 * T - 1, &occupied);
    assert_eq!(banked.psg_snapshot().volumes, [15; 4]);
    advance(&mut banked, 43 * T - 1, 43 * T, &occupied);
    assert_eq!(banked.psg_snapshot().volumes, [0, 15, 15, 15]);
    assert_eq!(banked.sound_cpu.cycles(), 73);

    let mut local = ready(
        &[
            0x3e, 0x90, 0x32, 0x11, 0x7f, 0x32, 0, 0x60, 0x3e, 0x55, 0x32, 0, 1, 0x76,
        ],
        0x1ff,
    );
    advance(&mut local, 0, 53 * T, &[vdp(0, 1000 * T)]);
    assert_eq!(local.psg_snapshot().volumes, [0, 15, 15, 15]);
    assert_eq!(local.bus.sound.bank, 0xff);
    assert_eq!(local.bus.sound.ram[0x100], 0x55);
    assert_eq!(local.sound_cpu.cycles(), 53);
    assert_eq!(local.sound_fault(), None);
}

#[test]
fn unqualified_vdp_wait_overflow_is_atomic_and_discards_only_pending_work() {
    let mut machine = ldi_pending(0x8000, 0x1ff, 0x55);
    let original = machine.sound_schedule.pending.as_ref().unwrap();
    let mut pending = original.clone();
    assert_eq!(
        pending.reserve_after(&[vdp(10 * T, u64::MAX)]),
        Err(SoundFault::ClockOverflow)
    );
    assert_eq!(&pending, original);
    pending.instruction.cycles = u32::MAX;
    let saved = pending.clone();
    assert_eq!(
        pending.reserve_after(&[vdp(10 * T, 40 * T)]),
        Err(SoundFault::ClockOverflow)
    );
    assert_eq!(pending, saved);

    let cpu = machine.sound_cpu.clone();
    let memory = machine.bus.sound.clone();
    let state = machine.sound_schedule.state;
    machine.bus.ram[0x8004] = 0xa6;
    advance(
        &mut machine,
        11 * T + 1,
        11 * T + 2,
        &[vdp(10 * T, u64::MAX)],
    );
    assert_eq!(machine.sound_fault(), Some(SoundFault::ClockOverflow));
    assert!(machine.sound_schedule.pending.is_none());
    assert_eq!(machine.sound_schedule.state, state);
    assert_eq!(machine.sound_cpu, cpu);
    assert_eq!(machine.bus.sound, memory);
    assert_eq!(machine.bus.ram[0x8000], 0xde);
    assert_eq!(machine.bus.ram[0x8004], 0xa6);
}

#[test]
fn prior_ldi_bank_write_waits_through_actual_dma_acquisition() {
    const SOURCE: usize = 0x8000;
    const DESTINATION: usize = 0x400;
    let region = Region::default();
    for h40 in [false, true] {
        let mut machine = super::vdp_line::seed(true, 0x4e71);
        for (index, value) in [
            (12, if h40 { 0x81 } else { 0 }),
            (1, 0x54),
            (19, 17),
            (20, 0),
            (21, 0),
            (22, 0xc0),
            (23, 0x7f),
        ] {
            machine.bus.advance_machine(1).unwrap();
            let write = machine
                .bus
                .vdp
                .queue_register(index, value, region)
                .unwrap();
            machine
                .bus
                .advance_machine(write.publish_at.get() - machine.bus.now.get())
                .unwrap();
        }
        let base = machine.bus.now.get().div_ceil(T) * T;
        machine
            .bus
            .advance_machine(base - machine.bus.now.get())
            .unwrap();
        machine.bus.sound.set_reset(false);
        machine.bus.sound.bank = 0x1ff;
        machine.bus.sound.ram[..12]
            .copy_from_slice(&[0x21, 0, 1, 0x11, 0, 0x80, 0x01, 1, 0, 0xed, 0xa0, 0x76]);
        machine.bus.sound.ram[0x100] = 0x55;
        machine.bus.ram[SOURCE..SOURCE + 34].fill(0x44);
        let mut snapshot = machine.bus.sound.clone();
        let mut setup = CandidateBus::new(&mut snapshot, &machine.bus.cartridge);
        for _ in 0..3 {
            machine.sound_cpu.step_with_bus(&mut setup).unwrap();
        }
        assert_eq!(setup.fault.get(), None);
        super::vdp_row_delivery::advance(&mut machine, 11 * T + 1).unwrap();
        let pending = machine.sound_schedule.pending.as_ref().unwrap().clone();
        assert_eq!((pending.start, pending.deadline), (base + 165, base + 210));
        assert!(pending.any_bank_access);
        assert!(pending.reservation().is_none());
        assert_eq!(machine.bus.now, machine.bus.vdp.now());
        machine.bus.vdp.write_control(0x4400, region).unwrap();

        let before = machine.bus.now.get();
        let acquisition = before + u64::from(TRANSFER_CLOCKS) * MAIN_CPU_DIVISOR;
        assert!(pending.start < acquisition && acquisition < pending.deadline);
        let mut probe = machine.clone();
        probe.bus.advance_machine(acquisition - before).unwrap();
        probe.bus.vdp.write_control(0x80, region).unwrap();
        let release = probe
            .vdp()
            .external_dma_release(
                region,
                VdpDmaSource::new(&probe.bus.cartridge.bytes, &probe.bus.ram),
            )
            .unwrap()
            .unwrap()
            .get();
        let main_waits = (release - acquisition).div_ceil(MAIN_CPU_DIVISOR);
        let sound_waits = (release - pending.start).div_ceil(T);
        let deadline = pending.deadline + sound_waits * T;
        let cpu = machine.sound_cpu.clone();
        let state = machine.sound_schedule.state;
        let mut copy = machine.clone();
        for candidate in [&mut machine, &mut copy] {
            let clock = candidate.bus.cpu_clock;
            let result = ScheduledBus {
                bus: &mut candidate.bus,
                cpu: &mut candidate.sound_cpu,
                schedule: &mut candidate.sound_schedule,
            }
            .write_cycle(
                BusAccess {
                    logical_address: 0xc00004,
                    bus_address: 0xc00004,
                    kind: AccessKind::Data,
                    lanes: ByteLanes::Both,
                    function_code: FunctionCode::SupervisorData,
                },
                0x80,
                clock,
            );
            assert_eq!(result.result, Ok(()));
            assert_eq!(u64::from(result.wait_clocks), main_waits);
            assert_eq!(
                candidate.bus.cpu_clock.0 - clock.0,
                u64::from(TRANSFER_CLOCKS) + main_waits
            );
            assert_eq!(
                candidate.bus.now.get(),
                acquisition + main_waits * MAIN_CPU_DIVISOR
            );
            assert_eq!(candidate.bus.now, candidate.bus.vdp.now());
            assert_eq!(candidate.sound_cpu, cpu);
            assert_eq!(candidate.sound_schedule.state, state);
            assert_eq!(candidate.bus.ram[SOURCE], 0x44);
            assert_eq!(candidate.vdp().registers().registers()[19], Some(0));
            assert_eq!(candidate.vdp().registers().registers()[21], Some(17));
            assert_eq!(candidate.vdp().registers().registers()[22], Some(0xc0));
            assert_eq!(candidate.vdp().registers().registers()[23], Some(0x7f));
            assert_eq!(
                candidate
                    .vdp()
                    .external_dma_release(
                        region,
                        VdpDmaSource::new(&candidate.bus.cartridge.bytes, &candidate.bus.ram),
                    )
                    .unwrap(),
                None
            );
            assert!(!candidate.vdp().fifo().is_empty());
            let held = candidate.sound_schedule.pending.as_ref().unwrap();
            assert_eq!(held.start, pending.start + sound_waits * T);
            assert!(held.start >= release);
            assert_eq!(held.deadline, deadline);
            assert_eq!(held.state, pending.state);
            assert_eq!(u64::from(held.instruction.cycles), 3 + sound_waits);
            assert_eq!(held.cpu.cycles(), pending.cpu.cycles() + sound_waits);
            let delta = deadline - 1 - candidate.bus.now.get();
            super::vdp_row_delivery::advance(candidate, delta).unwrap();
            assert_eq!(candidate.bus.ram[SOURCE], 0x44);
            assert_eq!(candidate.sound_cpu, cpu);
            super::vdp_row_delivery::advance(candidate, 1).unwrap();
            assert_eq!(candidate.bus.ram[SOURCE], 0x55);
            assert_eq!(
                candidate.sound_cpu.cycles(),
                pending.cpu.cycles() + sound_waits
            );
            super::vdp_row_delivery::advance(candidate, 2000).unwrap();
            assert_eq!(
                &candidate.vdp().memory().vram()[DESTINATION..DESTINATION + 34],
                &[0x44; 34]
            );
            assert_eq!(candidate.bus.ram[SOURCE], 0x55);
            assert_eq!(candidate.sound_fault(), None);
        }
        assert_eq!(machine, copy);
    }
}
