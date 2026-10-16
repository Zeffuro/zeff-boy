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
    let (cartridge, mut cpu, mut memory, mut audio, mut schedule) = setup(0x4000);
    audio.write_opn2(0, 0, 0x2a);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(0..151, &mut audio),
    );
    assert_eq!(schedule.pending.as_ref().unwrap().deadline, 195);
    assert_eq!(cpu.regs().a, 0);
    audio.write_opn2(160, 1, 0x90);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(160..195, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.regs().a, 0x80);
    assert_eq!(cpu.cycles(), 13);
}

#[test]
fn status_read_observes_busy_expiry_within_read_phase() {
    let (cartridge, mut cpu, mut memory, mut audio, mut schedule) = setup(0x4000);
    audio.write_opn2(0, 0, 0x2a);
    audio.write_opn2(0, 1, 0x90);
    audio.advance_to(1185);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(1185..1336, &mut audio),
    );
    assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1380);
    assert_eq!(audio.opn2_status_at(1336), 0x80);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(1336..1380, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.regs().a, 0);
}

#[test]
fn banked_status_read_resamples_after_reservation_waits() {
    let (cartridge, mut cpu, mut memory, mut audio, mut schedule) = setup(0xc000);
    memory.bank = 0x140;
    audio.write_opn2(0, 0, 0x2a);
    audio.write_opn2(0, 1, 0x90);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
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
        &cartridge,
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
        pending.refresh_opn2(&cpu, &memory, &cartridge, &audio),
        Err(SoundFault::UnstableOpn2Read { pc: 0x4000 })
    );
    assert_eq!(pending, before);
    assert_eq!(audio, original);
    assert_eq!(cpu.cycles(), 0);
}

#[test]
fn pending_status_read_observes_timer_flag_clear() {
    let (cartridge, mut cpu, mut memory, mut audio, mut schedule) = setup(0x4000);
    for (register, value) in [(0x24, 0xff), (0x25, 3), (0x27, 5)] {
        audio.write_opn2(0, 0, register);
        audio.write_opn2(0, 1, value);
    }
    audio.advance_to(1134);
    assert_eq!(audio.opn2_status_at(1134) & 1, 1);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(1134..1286, &mut audio),
    );
    audio.write_opn2(1286, 0, 0x27);
    audio.write_opn2(1286, 1, 0x10);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(1286..1326, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.regs().a, 0x80);
}

#[test]
fn timer_overflow_is_sampled_at_final_read_completion() {
    for (start, expected) in [(930, 0x80), (945, 0x81)] {
        let (cartridge, mut cpu, mut memory, mut audio, mut schedule) = setup(0x4000);
        for (register, value) in [(0x24, 0xff), (0x25, 3), (0x27, 5)] {
            audio.write_opn2(0, 0, register);
            audio.write_opn2(0, 1, value);
        }
        audio.advance_to(start);
        schedule.advance(
            &mut cpu,
            &mut memory,
            &cartridge,
            &mut [],
            SoundInterval::new(start..start + 195, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        assert_eq!(cpu.regs().a, expected);
    }
}

#[test]
fn status_read_modify_write_replays_from_original_cpu_and_publishes_once() {
    let (cartridge, mut cpu, mut memory, mut audio, _) = setup(0x4000);
    memory.ram[..5].copy_from_slice(&[0x21, 0, 0x40, 0x34, 0x76]);
    PendingSound::prepare(&cpu, &memory, &cartridge, 0)
        .unwrap()
        .publish(&mut cpu, &mut memory, &mut [], &mut audio);
    let mut pending = PendingSound::prepare(&cpu, &memory, &cartridge, 150).unwrap();
    audio.write_opn2(151, 0, 0x2a);
    audio.write_opn2(151, 1, 0x90);
    let before = audio.clone();
    pending
        .refresh_opn2(&cpu, &memory, &cartridge, &audio)
        .unwrap();
    assert_eq!(audio, before);
    assert_eq!(cpu.regs().pc, 3);
    pending.publish(&mut cpu, &mut memory, &mut [], &mut audio);
    assert_eq!(audio.opn2.debug_snapshot().address, 0x81);
    assert_eq!(audio.opn2.debug_snapshot().dac, 0x90);
    assert_eq!(cpu.regs().pc, 4);
    assert_eq!(cpu.regs().f & 0x80, 0x80);
}
