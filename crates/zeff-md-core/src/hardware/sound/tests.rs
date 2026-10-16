use super::schedule::{PendingSound, SoundInterval, SoundSchedule};
use super::*;
use crate::hardware::bus::Bus;
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::hardware::bus::scheduled::ScheduledBus;
use zeff_emu_common::time::MasterTicks;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;
use zeff_z80::Cpu;

fn cartridge() -> Cartridge {
    let mut bytes = vec![0; 16];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 0, 8]);
    Cartridge::new(bytes).unwrap()
}

fn pending_store(value: u8) -> (Cartridge, Cpu, SoundMemory, Audio, PendingSound) {
    let cartridge = cartridge();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.ram[..6].copy_from_slice(&[0x3e, value, 0x32, 0x11, 0x7f, 0x76]);
    let mut cpu = Cpu::new();
    let mut audio = Audio::default();
    PendingSound::prepare(&cpu, &memory, &cartridge, 0)
        .unwrap()
        .publish(&mut cpu, &mut memory, &mut [], &mut audio);
    let pending = PendingSound::prepare(&cpu, &memory, &cartridge, 105).unwrap();
    assert_eq!(pending.deadline, 300);
    (cartridge, cpu, memory, audio, pending)
}

#[test]
fn direct_psg_window_and_byte_parity_are_exact() {
    let cartridge = cartridge();
    for address in 0x7f00..=0x7fff {
        let mut memory = SoundMemory::default();
        let mut candidate = CandidateBus::new(&mut memory, &cartridge);
        candidate.cpu_write(address, 0x82);
        let accepted = (SOUND_PSG_START..=SOUND_PSG_END).contains(&address);
        if accepted {
            assert_eq!(candidate.fault.get(), None);
            assert_eq!(
                candidate.psg_writes.0[0],
                (address & 1 != 0).then_some(0x82)
            );
        } else {
            assert_eq!(
                candidate.fault.get(),
                Some(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Write,
                })
            );
        }
        let mut memory = SoundMemory::default();
        let reader = CandidateBus::new(&mut memory, &cartridge);
        assert_eq!(reader.cpu_read(address), UNPOPULATED_ROM_BYTE);
        assert_eq!(
            reader.fault.get(),
            Some(SoundFault::UnsupportedAccess {
                address,
                direction: Direction::Read,
            })
        );
    }
}

#[test]
fn psg_journal_is_bounded_ordered_and_unpublished() {
    let cartridge = cartridge();
    let mut memory = SoundMemory::default();
    let mut candidate = CandidateBus::new(&mut memory, &cartridge);
    let audio = Audio::default();
    let before = audio.clone();
    candidate.cpu_write(SOUND_PSG_START + 1, 0x82);
    candidate.cpu_write(SOUND_PSG_END, 0x15);
    assert_eq!(audio, before);
    assert_eq!(candidate.fault.get(), None);
    let accepted = candidate.psg_writes;
    candidate.cpu_write(SOUND_PSG_START + 3, 0x9f);
    assert_eq!(
        candidate.fault.get(),
        Some(SoundFault::UnsupportedAccess {
            address: SOUND_PSG_START + 3,
            direction: Direction::Write,
        })
    );
    assert_eq!(candidate.psg_writes, accepted);
    let mut published = audio;
    accepted.publish(&mut published, 300);
    assert_eq!(published.psg.debug_snapshot().periods[0], 0x152);
}

#[test]
fn fault_after_first_psg_write_rejects_entire_candidate() {
    let cartridge = cartridge();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.ram[..6].copy_from_slice(&[0x21, 0x82, 0x15, 0x22, 0x17, 0x7f]);
    let mut cpu = Cpu::new();
    let mut audio = Audio::default();
    PendingSound::prepare(&cpu, &memory, &cartridge, 0)
        .unwrap()
        .publish(&mut cpu, &mut memory, &mut [], &mut audio);
    let before = audio.clone();
    let saved_cpu = cpu.clone();
    let saved_memory = memory.clone();
    assert_eq!(
        PendingSound::prepare(&cpu, &memory, &cartridge, 150),
        Err(SoundFault::UnsupportedAccess {
            address: 0x7f18,
            direction: Direction::Write,
        })
    );
    assert_eq!(audio, before);
    assert_eq!(cpu, saved_cpu);
    assert_eq!(memory, saved_memory);
}

#[test]
fn banked_psg_decode_captures_exact_main_bus_reservation() {
    let cartridge = cartridge();
    for (bank, address, accepted) in [
        (0x180, 0x8010, true),
        (0x180, 0x8011, true),
        (0x180, 0x8017, true),
        (0x180, 0x8018, false),
        (0x180, 0x8031, false),
        (0x188, 0x8011, false),
    ] {
        let mut memory = SoundMemory {
            bank,
            ..SoundMemory::default()
        };
        let mut candidate = CandidateBus::new(&mut memory, &cartridge);
        candidate.cpu_write(address, 0x82);
        assert_eq!(candidate.fault.get().is_none(), accepted);
        if accepted {
            assert_eq!(
                candidate.bank_access.get(),
                Some(BankAccess {
                    address: u32::from(bank) << SOUND_BANK_SHIFT | u32::from(address & 0x7fff),
                    direction: Direction::Write,
                    value: 0x82,
                })
            );
            assert_eq!(
                candidate.psg_writes.0[0],
                (address & 1 != 0).then_some(0x82)
            );
        }
    }
    let mut memory = SoundMemory {
        bank: 0x180,
        ..SoundMemory::default()
    };
    memory.set_reset(false);
    memory.ram[..6].copy_from_slice(&[0x3e, 0x82, 0x32, 0x11, 0x80, 0x76]);
    let mut cpu = Cpu::new();
    let mut audio = Audio::default();
    let mut schedule = SoundSchedule::default();
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(0..299, &mut audio),
    );
    let pending = schedule.pending.as_ref().unwrap();
    assert_eq!(pending.bank_access.unwrap().address, 0xc00011);
    assert_eq!(pending.reservation().unwrap().requester, Requester::SOUND);
    assert_eq!(pending.reservation().unwrap().ticks, 255..300);
}

#[test]
fn deadlines_publish_before_tail_and_preserve_later_main_registers() {
    let (cartridge, mut cpu, mut memory, mut audio, pending) = pending_store(0x82);
    let mut expected = audio.clone();
    let mut schedule = SoundSchedule {
        pending: Some(pending),
        ..SoundSchedule::default()
    };
    audio.write_psg(200, 0xa3);
    expected.write_psg(200, 0xa3);
    memory.request(true);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(200..299, &mut audio),
    );
    expected.advance_to(299);
    assert_eq!(audio, expected);
    assert!(schedule.pending.is_some());
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(299..300, &mut audio),
    );
    expected.write_psg(300, 0x82);
    assert_eq!(audio, expected);
    assert_eq!(audio.psg.debug_snapshot().periods[..2], [2, 3]);
    assert!(schedule.pending.is_none());
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(300..1000, &mut audio),
    );
    expected.advance_to(1000);
    assert_eq!(audio, expected);
}

#[test]
fn schedule_faults_clock_tail_and_discard_rejected_effects() {
    let (cartridge, mut cpu, mut memory, mut audio, mut pending) = pending_store(0x82);
    pending.bank_access = Some(BankAccess {
        address: 0xc00011,
        direction: Direction::Write,
        value: 0x82,
    });
    let mut expected = audio.clone();
    let mut schedule = SoundSchedule {
        pending: Some(pending),
        ..SoundSchedule::default()
    };
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval {
            sound_irq: None,
            audio: &mut audio,
            ticks: 105..1000,
            occupied: &[Reservation {
                requester: Requester::MAIN,
                ticks: 105..u64::MAX,
            }],
        },
    );
    expected.advance_to(1000);
    assert_eq!(audio, expected);
    assert_eq!(schedule.fault, Some(SoundFault::ClockOverflow));
    assert!(schedule.pending.is_none());
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(1000..2000, &mut audio),
    );
    expected.advance_to(2000);
    assert_eq!(audio, expected);
}

#[test]
fn sound_reset_discards_pending_effects_and_preserves_chip() {
    let (cartridge, mut cpu, mut memory, mut audio, pending) = pending_store(0x82);
    audio.write_psg(200, 0xa3);
    let mut expected = audio.clone();
    let mut schedule = SoundSchedule {
        pending: Some(pending),
        ..SoundSchedule::default()
    };
    memory.set_reset(true);
    schedule.reset_cpu(&mut cpu, &mut memory);
    assert!(schedule.pending.is_none());
    assert_eq!(audio, expected);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(200..1000, &mut audio),
    );
    expected.advance_to(1000);
    assert_eq!(audio, expected);
    assert_eq!(audio.psg.debug_snapshot().periods[..2], [0, 3]);
}

#[test]
fn scheduled_main_psg_transfer_obeys_before_tied_and_after_deadlines() {
    for end in [299, 300, 301] {
        let (cartridge, mut cpu, memory, audio, pending) = pending_store(0x82);
        let mut expected = audio.clone();
        let mut bus = Bus::new(cartridge, crate::Region::default());
        bus.sound = memory;
        bus.sound.request(true);
        bus.audio = audio;
        bus.now = MasterTicks::new(end - 28);
        let mut schedule = SoundSchedule {
            pending: Some(pending),
            ..SoundSchedule::default()
        };
        if end >= 300 {
            expected.write_psg(300, 0x82);
        }
        expected.write_psg(end, 0xa3);
        let clock = bus.cpu_clock;
        let result = ScheduledBus {
            bus: &mut bus,
            cpu: &mut cpu,
            schedule: &mut schedule,
        }
        .write_cycle(
            BusAccess {
                logical_address: 0xc00011,
                bus_address: 0xc00010,
                function_code: FunctionCode::SupervisorData,
                kind: AccessKind::Data,
                lanes: ByteLanes::Lower,
            },
            0xa3,
            clock,
        );
        assert_eq!(result.result, Ok(()));
        assert_eq!(result.wait_clocks, 0);
        assert_eq!(bus.audio, expected);
        if end < 300 {
            expected.write_psg(300, 0x82);
            ScheduledBus {
                bus: &mut bus,
                cpu: &mut cpu,
                schedule: &mut schedule,
            }
            .advance_machine(1)
            .unwrap();
            assert_eq!(bus.audio, expected);
        }
        assert_eq!(bus.audio.psg.debug_snapshot().periods[..2], [2, 3]);
        assert_eq!(
            bus.audio.psg.debug_snapshot().latched_register,
            if end < 300 { 0 } else { 2 }
        );
    }
}
