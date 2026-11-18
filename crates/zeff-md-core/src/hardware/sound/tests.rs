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
    let mut cartridge = cartridge();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.ram[..6].copy_from_slice(&[0x3e, value, 0x32, 0x11, 0x7f, 0x76]);
    let mut cpu = Cpu::new();
    let mut audio = Audio::default();
    PendingSound::prepare(&cpu, &memory, &cartridge, 0)
        .unwrap()
        .publish(&mut cpu, &mut memory, &mut [], &mut audio, &mut cartridge);
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
    let mut cartridge = cartridge();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.ram[..6].copy_from_slice(&[0x21, 0x82, 0x15, 0x22, 0x17, 0x7f]);
    let mut cpu = Cpu::new();
    let mut audio = Audio::default();
    PendingSound::prepare(&cpu, &memory, &cartridge, 0)
        .unwrap()
        .publish(&mut cpu, &mut memory, &mut [], &mut audio, &mut cartridge);
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
    let mut cartridge = cartridge();
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
        &mut cartridge,
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
    let (mut cartridge, mut cpu, mut memory, mut audio, pending) = pending_store(0x82);
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
        &mut cartridge,
        &mut [],
        SoundInterval::new(200..299, &mut audio),
    );
    expected.advance_to(299);
    assert_eq!(audio, expected);
    assert!(schedule.pending.is_some());
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
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
        &mut cartridge,
        &mut [],
        SoundInterval::new(300..1000, &mut audio),
    );
    expected.advance_to(1000);
    assert_eq!(audio, expected);
}

#[test]
fn schedule_faults_clock_tail_and_discard_rejected_effects() {
    let (mut cartridge, mut cpu, mut memory, mut audio, mut pending) = pending_store(0x82);
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
        &mut cartridge,
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
        &mut cartridge,
        &mut [],
        SoundInterval::new(1000..2000, &mut audio),
    );
    expected.advance_to(2000);
    assert_eq!(audio, expected);
}

#[test]
fn sound_reset_discards_pending_effects_and_preserves_chip() {
    let (mut cartridge, mut cpu, mut memory, mut audio, pending) = pending_store(0x82);
    audio.write_psg(200, 0xa3);
    let mut expected = audio.clone();
    let mut schedule = SoundSchedule {
        pending: Some(pending),
        ..SoundSchedule::default()
    };
    memory.set_reset(true);
    schedule.reset_cpu(
        &mut cpu,
        &mut memory,
        &mut crate::sound_trace::NoSoundCapture,
    );
    assert!(schedule.pending.is_none());
    assert_eq!(audio, expected);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
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
        let result = ScheduledBus::new(&mut bus, &mut cpu, &mut schedule).write_cycle(
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
            ScheduledBus::new(&mut bus, &mut cpu, &mut schedule)
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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unused_cartridge_reads_capture_bank_identity_without_accepting_writes_or_lockup_space() {
    normal_slot_banked_projection();
    inserted_save_accesses();
    let cartridge = cartridge();
    for bank in 0x80..=0xff {
        for address in [0x8000, 0x8001, 0xfffe, 0xffff] {
            let mut memory = SoundMemory {
                bank,
                ..SoundMemory::default()
            };
            let before = memory.clone();
            let mut bus = CandidateBus::new(&mut memory, &cartridge);
            assert_eq!(bus.cpu_read(address), 0xff);
            assert_eq!(bus.fault.get(), None);
            assert_eq!(
                bus.bank_access.get(),
                Some(BankAccess {
                    address: (u32::from(bank) << 15) | u32::from(address & 0x7fff),
                    direction: Direction::Read,
                    value: 0xff,
                })
            );
            assert!(!bus.timed_read.get());
            bus.cpu_write(address, 0x42);
            assert_eq!(
                bus.fault.get(),
                Some(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Write,
                })
            );
            assert_eq!(memory, before);
        }
    }
    for target in [0x800000, 0x9fffff, 0xa00000, 0xa17fff, 0xbfffff, 0xdfffff] {
        let address = (0x8000 | (target & 0x7fff)) as u16;
        let mut memory = SoundMemory {
            bank: (target >> 15) as u16,
            ..SoundMemory::default()
        };
        let before = memory.clone();
        let bus = CandidateBus::new(&mut memory, &cartridge);
        assert_eq!(bus.cpu_read(address), 0xff);
        assert_eq!(bus.bank_access.get(), None);
        assert_eq!(
            bus.fault.get(),
            Some(SoundFault::UnsupportedAccess {
                address,
                direction: Direction::Read,
            })
        );
        assert_eq!(memory, before);
    }
}

fn normal_slot_banked_projection() {
    use crate::cartridge::registers::LockOn::*;
    for mapping in [Empty, HalfMiB, MiB, TwoMiB] {
        let mut cartridge = Cartridge::new(crate::cartridge::registers::tests::normal_rom_fixture(
            mapping,
        ))
        .unwrap();
        cartridge.lock_on = Some(mapping);
        for target in [
            0x1fffff_u32,
            0x200000,
            0x27ffff,
            0x280000,
            0x300000,
            0x33ffff,
            0x380000,
            0x3fffff,
        ] {
            let mut memory = SoundMemory {
                bank: (target >> 15) as u16,
                ..SoundMemory::default()
            };
            let reader = CandidateBus::new(&mut memory, &cartridge);
            assert_eq!(
                reader.cpu_read(0x8000 | (target & 0x7fff) as u16),
                cartridge.read(target).unwrap()
            );
            assert_eq!(reader.fault.get(), None);
            let mutable = mapping.mode_sensitive(target);
            assert_eq!(
                reader.cartridge_read.get(),
                mutable.then_some(0x8000 | (target & 0x7fff) as u16)
            );
            assert_eq!(reader.timed_read.get(), mutable);
        }
    }
}

fn inserted_save_accesses() {
    use crate::cartridge::registers::{LockOn, LockOnSave};
    let cells: Vec<_> = (0..512).map(|index| index as u8 ^ 0xa5).collect();
    for enabled in [false, true] {
        let mut cartridge = Cartridge::new(crate::cartridge::registers::tests::normal_rom_fixture(
            LockOn::TwoMiB,
        ))
        .unwrap();
        cartridge.lock_on = Some(LockOn::TwoMiB);
        let mut save = LockOnSave::new();
        save.load(&cells).unwrap();
        save.enabled = enabled;
        cartridge.lock_on_save = Some(save);
        for target in [
            0x1fffff, 0x200000, 0x200001, 0x2003fe, 0x2003ff, 0x200400, 0x202000, 0x2fffff,
            0x300000, 0x3fffff,
        ] {
            let address = 0x8000 | (target & 0x7fff) as u16;
            let sensitive = (0x200000..=0x2003ff).contains(&target) || target >= 0x300000;
            let expected = if enabled && sensitive {
                (target < 0x300000 && target & 1 != 0)
                    .then(|| cells[((target & 0x3ff) >> 1) as usize])
            } else {
                Some(cartridge.bytes[target as usize])
            };
            let mut memory = SoundMemory {
                bank: (target >> 15) as u16,
                ..SoundMemory::default()
            };
            let before = memory.clone();
            let mut reader = CandidateBus::new(&mut memory, &cartridge);
            assert_eq!(reader.cpu_read(address), expected.unwrap_or(0xff));
            assert_eq!(
                reader.fault.get(),
                expected.is_none().then_some(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Read
                })
            );
            assert_eq!(reader.timed_read.get(), sensitive);
            assert_eq!(reader.cartridge_read.get(), sensitive.then_some(address));
            reader.fault.set(None);
            reader.cpu_write(address, 0x42);
            assert_eq!(
                reader.fault.get(),
                sensitive.then_some(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Write
                })
            );
            assert_eq!(reader.writes, WorkWrites::default());
            assert_eq!(reader.register_writes, RegisterWrites::default());
            assert_eq!(reader.sf001_writes, Sf001Writes::default());
            assert_eq!(memory, before);
            assert_eq!(cartridge.lock_on_save.as_ref().unwrap().bytes(), cells);
        }
    }
}
