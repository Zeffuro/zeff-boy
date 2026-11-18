use super::schedule::{PendingSound, SoundInterval, SoundSchedule};
use super::*;
use crate::hardware::bus::ownership::{Requester, Reservation};
use zeff_z80::{Cpu, ResetState};

fn setup(address: u16) -> (Cartridge, Cpu, SoundMemory, Audio, SoundSchedule) {
    let mut bytes = vec![0; 16];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 0, 8]);
    let cartridge = Cartridge::new(bytes).unwrap();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    let [low, high] = address.to_le_bytes();
    memory.ram[..4].copy_from_slice(&[0x3a, low, high, 0x76]);
    (
        cartridge,
        Cpu::new(),
        memory,
        Audio::default(),
        SoundSchedule::default(),
    )
}

#[test]
fn status_read_observes_data_write_after_preparation() {
    let (mut cartridge, mut cpu, mut memory, mut audio, mut schedule) = setup(0x4000);
    audio.write_opn2(0, 0, 0x2a);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(0..151, &mut audio),
    );
    assert_eq!(schedule.pending.as_ref().unwrap().deadline, 195);
    assert_eq!(cpu.regs().a, 0);
    audio.write_opn2(160, 1, 0x90);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(160..195, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.regs().a, 0x80);
    assert_eq!(cpu.cycles(), 13);
}

#[test]
fn status_read_observes_busy_expiry_within_read_phase() {
    let (mut cartridge, mut cpu, mut memory, mut audio, mut schedule) = setup(0x4000);
    audio.write_opn2(0, 0, 0x2a);
    audio.write_opn2(0, 1, 0x90);
    audio.advance_to(1185);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(1185..1336, &mut audio),
    );
    assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1380);
    assert_eq!(audio.opn2_status_at(1336), 0x80);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(1336..1380, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.regs().a, 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_status_read_resamples_after_reservation_waits() {
    indexed_status_reservations_preserve_legacy_replay();
    let (mut cartridge, mut cpu, mut memory, mut audio, mut schedule) = setup(0xc000);
    memory.bank = 0x140;
    audio.write_opn2(0, 0, 0x2a);
    audio.write_opn2(0, 1, 0x90);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(0..151, &mut audio),
    );
    assert_eq!(
        schedule
            .pending
            .as_ref()
            .unwrap()
            .bank_access
            .unwrap()
            .address,
        0xa04000
    );
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval {
            sound_irq: None,
            ticks: 151..1545,
            occupied: &[Reservation {
                requester: Requester::MAIN,
                ticks: 150..1500,
            }],
            audio: &mut audio,
        },
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.regs().a, 0);
    assert_eq!(cpu.cycles(), 103);
    assert!(schedule.pending.is_none());
}

fn indexed_status_reservations_preserve_legacy_replay() {
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    {
        for legacy in [false, true] {
            let (mut cartridge, mut cpu, mut memory, mut audio, mut schedule) = setup(0xc000);
            memory.bank = 0x140;
            memory.ram[..8].copy_from_slice(&[0xfd, 0x21, 0, 0xc0, 0xfd, 0x66, 0, 0x76]);
            audio.write_opn2(0, 0, 0x2a);
            audio.write_opn2(0, 1, 0x90);
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cartridge,
                &mut [],
                SoundInterval::new(0..451, &mut audio),
            );
            assert_eq!(schedule.fault, None);
            let pending = schedule.pending.as_mut().unwrap();
            assert_eq!((pending.start, pending.deadline), (450, 495));
            assert_eq!(pending.bank_access.unwrap().address, 0xa04000);
            if legacy {
                pending.bank_access = None;
            }
            memory.request(true);
            let mut writer = StateWriter::new();
            schedule.write_state(&cpu, &mut writer).unwrap();
            let bytes = writer.into_bytes();
            let mut restored_cpu = Cpu::new();
            let mut restored = SoundSchedule::default();
            restored
                .read_state(&mut restored_cpu, &mut StateReader::new(&bytes))
                .unwrap();
            restored.validate_cartridge(&cartridge).unwrap();
            let mut writer = StateWriter::new();
            restored.write_state(&restored_cpu, &mut writer).unwrap();
            assert_eq!(writer.into_bytes(), bytes);
            assert_eq!(
                restored.pending.as_ref().unwrap().reservation().is_none(),
                legacy
            );
            let mut other_memory = memory.clone();
            let mut other_audio = audio.clone();
            let end = if legacy { 495 } else { 1545 };
            for (cpu, memory, schedule, audio) in [
                (&mut cpu, &mut memory, &mut schedule, &mut audio),
                (
                    &mut restored_cpu,
                    &mut other_memory,
                    &mut restored,
                    &mut other_audio,
                ),
            ] {
                schedule.advance(
                    cpu,
                    memory,
                    &mut cartridge,
                    &mut [],
                    SoundInterval {
                        ticks: 451..end,
                        occupied: &[Reservation {
                            requester: Requester::MAIN,
                            ticks: 450..1500,
                        }],
                        audio,
                        sound_irq: None,
                    },
                );
                assert_eq!(schedule.fault, None);
                assert!(schedule.pending.is_none());
                assert_eq!(cpu.regs().h, if legacy { 0x80 } else { 0 });
                assert_eq!(cpu.cycles(), if legacy { 33 } else { 103 });
                assert!(memory.granted);
            }
            assert_eq!(
                (cpu, memory, schedule, audio),
                (restored_cpu, other_memory, restored, other_audio)
            );
        }
    }
}

#[test]
fn banked_opn2_aperture_mirror_preserves_physical_identity() {
    let (cartridge, _, mut memory, _, _) = setup(0xc000);
    for bank in [0x140, 0x141] {
        memory.bank = bank;
        for port in 0..4 {
            let mut snapshot = memory.clone();
            let mut candidate = CandidateBus::new(&mut snapshot, &cartridge);
            candidate.opn2_status = 0x83;
            assert_eq!(
                candidate.cpu_read(0xc000 + port),
                if port == 0 { 0x83 } else { 0 }
            );
            assert_eq!(
                candidate.bank_access.get().unwrap().address,
                (u32::from(bank) << 15) | 0x4000 | u32::from(port)
            );
            assert_eq!(candidate.fault.get(), None);
            let mut snapshot = memory.clone();
            let mut writer = CandidateBus::new(&mut snapshot, &cartridge);
            writer.cpu_write(0xc000 + port, 0x92);
            assert_eq!(writer.opn2_writes.0[0], Some((port as u8, 0x92)));
            assert_eq!(
                writer.bank_access.get().unwrap().address,
                candidate.bank_access.get().unwrap().address
            );
            assert_eq!(writer.fault.get(), None);
        }
    }
}

#[test]
fn changed_status_opcode_rejects_refresh_without_publication() {
    let (cartridge, _, memory, mut audio, _) = setup(0x4000);
    let cpu = Cpu::new_with_reset(ResetState::new(0x4000, 0x1ffe));
    let mut pending = PendingSound::prepare(&cpu, &memory, &cartridge, 0).unwrap();
    let before = pending.clone();
    audio.write_opn2(0, 0, 0x2a);
    audio.write_opn2(0, 1, 0x90);
    let original = audio.clone();
    assert_eq!(
        pending.refresh_reads(&cpu, &memory, &cartridge, &audio),
        Err(SoundFault::UnstableOpn2Read { pc: 0x4000 })
    );
    assert_eq!(pending, before);
    assert_eq!(audio, original);
    assert_eq!(cpu.cycles(), 0);
}

#[test]
fn pending_status_read_observes_timer_flag_clear() {
    let (mut cartridge, mut cpu, mut memory, mut audio, mut schedule) = setup(0x4000);
    for (register, value) in [(0x24, 0xff), (0x25, 3), (0x27, 5)] {
        audio.write_opn2(0, 0, register);
        audio.write_opn2(0, 1, value);
    }
    audio.advance_to(1134);
    assert_eq!(audio.opn2_status_at(1134) & 1, 1);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(1134..1286, &mut audio),
    );
    audio.write_opn2(1286, 0, 0x27);
    audio.write_opn2(1286, 1, 0x10);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(1286..1326, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.regs().a, 0x80);
}

#[test]
fn timer_overflow_is_sampled_at_final_read_completion() {
    for (start, expected) in [(930, 0x80), (945, 0x81)] {
        let (mut cartridge, mut cpu, mut memory, mut audio, mut schedule) = setup(0x4000);
        for (register, value) in [(0x24, 0xff), (0x25, 3), (0x27, 5)] {
            audio.write_opn2(0, 0, register);
            audio.write_opn2(0, 1, value);
        }
        audio.advance_to(start);
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(start..start + 195, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        assert_eq!(cpu.regs().a, expected);
    }
}

#[test]
fn status_read_modify_write_replays_from_original_cpu_and_publishes_once() {
    let (mut cartridge, mut cpu, mut memory, mut audio, _) = setup(0x4000);
    memory.ram[..5].copy_from_slice(&[0x21, 0, 0x40, 0x34, 0x76]);
    PendingSound::prepare(&cpu, &memory, &cartridge, 0)
        .unwrap()
        .publish(&mut cpu, &mut memory, &mut [], &mut audio, &mut cartridge);
    let mut pending = PendingSound::prepare(&cpu, &memory, &cartridge, 150).unwrap();
    audio.write_opn2(151, 0, 0x2a);
    audio.write_opn2(151, 1, 0x90);
    let before = audio.clone();
    pending
        .refresh_reads(&cpu, &memory, &cartridge, &audio)
        .unwrap();
    assert_eq!(audio, before);
    assert_eq!(cpu.regs().pc, 3);
    pending.publish(&mut cpu, &mut memory, &mut [], &mut audio, &mut cartridge);
    assert_eq!(audio.opn2.debug_snapshot().address, 0x81);
    assert_eq!(audio.opn2.debug_snapshot().dac, 0x90);
    assert_eq!(cpu.regs().pc, 4);
    assert_eq!(cpu.regs().f & 0x80, 0x80);
}

mod batch_tests;

#[test]
fn mirrored_ym_decode_preserves_ports_timed_reads_and_banked_identity() {
    let (cartridge, _, memory, _, _) = setup(0x4601);
    for offset in 0..0x2000 {
        let mut direct = memory.clone();
        let mut bus = CandidateBus::new(&mut direct, &cartridge);
        bus.opn2_status = 0x83;
        let port = (offset & 3) as u8;
        assert_eq!(
            bus.cpu_read(0x4000 + offset),
            if port == 0 { 0x83 } else { 0 }
        );
        assert!(bus.timed_read.get());
        bus.cpu_write(0x4000 + offset, 0x92);
        assert_eq!(bus.opn2_writes.0[0], Some((port, 0x92)));
        assert_eq!(bus.fault.get(), None);
        for bank in [0x140, 0x141] {
            let mut banked = memory.clone();
            banked.bank = bank;
            let mut bus = CandidateBus::new(&mut banked, &cartridge);
            if offset >= 4 {
                let address = 0xc000 + offset;
                assert_eq!(bus.cpu_read(address), 0xff);
                assert_eq!(
                    bus.fault.get(),
                    Some(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Read,
                    })
                );
                assert!(!bus.timed_read.get());
                assert_eq!(bus.bank_access.get(), None);
                let mut writer_memory = memory.clone();
                writer_memory.bank = bank;
                let mut writer = CandidateBus::new(&mut writer_memory, &cartridge);
                writer.cpu_write(address, 0x92);
                assert_eq!(
                    writer.fault.get(),
                    Some(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Write,
                    })
                );
                assert_eq!(writer.opn2_writes, Opn2Writes::default());
                continue;
            }
            bus.opn2_status = 0x83;
            assert_eq!(
                bus.cpu_read(0xc000 + offset),
                if port == 0 { 0x83 } else { 0 }
            );
            assert!(bus.timed_read.get());
            bus.cpu_write(0xc000 + offset, 0x92);
            assert_eq!(bus.opn2_writes.0[0], Some((port, 0x92)));
            assert_eq!(
                bus.bank_access.get().unwrap().address,
                (u32::from(bank) << 15) | 0x4000 | u32::from(offset)
            );
            assert_eq!(bus.fault.get(), None);
        }
    }
    for address in [0x6000, 0x60ff, 0x6100, 0x7eff] {
        let mut memory = memory.clone();
        let mut bus = CandidateBus::new(&mut memory, &cartridge);
        assert_eq!(bus.cpu_read(address), 0xff);
        assert!(!bus.timed_read.get());
        bus.cpu_write(address, 1);
        assert_eq!(bus.opn2_writes, Opn2Writes::default());
        assert_eq!(bus.fault.get(), None);
        assert_eq!(memory.bank, if address <= 0x60ff { 0x100 } else { 0 });
    }
}

#[test]
fn mirrored_status_pending_restore_resamples_after_writes_and_banked_waits() {
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    for banked in [false, true] {
        let (mut cartridge, mut cpu, mut memory, mut audio, mut schedule) =
            setup(if banked { 0xc000 } else { 0x4600 });
        memory.bank = if banked { 0x140 } else { 0 };
        audio.write_opn2(0, 0, 0x2a);
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(0..151, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        assert_eq!(
            schedule
                .pending
                .as_ref()
                .unwrap()
                .bank_access
                .map(|a| a.address),
            banked.then_some(0xa04000)
        );
        let mut writer = StateWriter::new();
        schedule.write_state(&cpu, &mut writer).unwrap();
        let mut restored = SoundSchedule::default();
        let mut restored_cpu = Cpu::new();
        restored
            .read_state(
                &mut restored_cpu,
                &mut StateReader::new(&writer.into_bytes()),
            )
            .unwrap();
        audio.write_opn2(160, 1, 0x90);
        let mut restored_memory = memory.clone();
        let mut restored_audio = audio.clone();
        let occupied = if banked {
            vec![Reservation {
                requester: Requester::MAIN,
                ticks: 150..1500,
            }]
        } else {
            vec![]
        };
        let end = if banked { 1545 } else { 195 };
        for (cpu, memory, schedule, audio) in [
            (&mut cpu, &mut memory, &mut schedule, &mut audio),
            (
                &mut restored_cpu,
                &mut restored_memory,
                &mut restored,
                &mut restored_audio,
            ),
        ] {
            schedule.advance(
                cpu,
                memory,
                &mut cartridge,
                &mut [],
                SoundInterval {
                    ticks: 160..end,
                    occupied: &occupied,
                    sound_irq: None,
                    audio,
                },
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(cpu.regs().a, if banked { 0 } else { 0x80 });
            assert!(schedule.pending.is_none());
        }
        assert_eq!(cpu, restored_cpu);
        assert_eq!(schedule, restored);
    }
}

#[test]
fn mirrored_ym_writes_publish_normalized_ports_only_at_completion() {
    let (mut cartridge, mut cpu, mut memory, mut audio, _) = setup(0x4600);
    memory.ram[..10].copy_from_slice(&[0x3e, 0x2a, 0x32, 0, 0x46, 0x3e, 0x90, 0x32, 0xfd, 0x5f]);
    for (start, end, expected) in [
        (0, 105, 0),
        (105, 300, 0x2a),
        (300, 405, 0x2a),
        (405, 600, 0x2a),
    ] {
        let pending = PendingSound::prepare(&cpu, &memory, &cartridge, start).unwrap();
        assert_eq!(pending.deadline, end);
        audio.advance_to(end - 1);
        assert_eq!(
            audio.opn2.debug_snapshot().address,
            if start < 300 { 0 } else { 0x2a }
        );
        assert_eq!(audio.opn2.debug_snapshot().dac, 0x80);
        pending.publish(&mut cpu, &mut memory, &mut [], &mut audio, &mut cartridge);
        assert_eq!(audio.opn2.debug_snapshot().address, expected);
        if end < 600 {
            assert_eq!(audio.opn2.debug_snapshot().dac, 0x80);
        }
    }
    assert_eq!(audio.opn2.debug_snapshot().dac, 0x90);
}
