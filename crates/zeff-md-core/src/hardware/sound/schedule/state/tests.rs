use super::*;

fn cartridge() -> Cartridge {
    let mut bytes = vec![0; 16];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 0, 8]);
    Cartridge::new(bytes).unwrap()
}

fn save(schedule: &SoundSchedule, cpu: &Cpu) -> Vec<u8> {
    let mut w = Writer::new();
    schedule.write_state(cpu, &mut w).unwrap();
    w.into_bytes()
}

fn restore(saved: &[u8]) -> (SoundSchedule, Cpu) {
    let mut cpu = Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut reader = Reader::new(saved);
    schedule.read_state(&mut cpu, &mut reader).unwrap();
    assert!(reader.is_exhausted());
    (schedule, cpu)
}

#[test]
fn pending_status_replay_continues_without_publication() {
    let cartridge = cartridge();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.bank = 0x140;
    memory.ram[..4].copy_from_slice(&[0x3a, 0x00, 0xc0, 0x76]);
    let mut cpu = Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut audio = Audio::default();
    audio.write_opn2(0, 0, 0x2a);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(0..151, &mut audio),
    );
    assert!(schedule.pending.as_ref().unwrap().replay.is_some());
    let saved = save(&schedule, &cpu);
    let (mut restored, mut restored_cpu) = restore(&saved);
    assert_eq!(restored, schedule);
    assert_eq!(restored_cpu, cpu);
    let mut restored_memory = memory.clone();
    let mut restored_audio = audio.clone();
    memory.bank = 0;
    memory.irq_asserted = true;
    restored_memory.bank = 0;
    restored_memory.irq_asserted = true;
    audio.write_opn2(160, 1, 0x90);
    restored_audio.write_opn2(160, 1, 0x90);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(160..600, &mut audio),
    );
    restored.advance(
        &mut restored_cpu,
        &mut restored_memory,
        &cartridge,
        &mut [],
        SoundInterval::new(160..600, &mut restored_audio),
    );
    assert_eq!(restored, schedule);
    assert_eq!(restored_cpu, cpu);
    assert_eq!(restored_memory, memory);
    assert_eq!(restored_audio, audio);
    assert_eq!(cpu.regs().a, 0x80);
}

#[test]
fn pending_journal_slots_and_sampled_memory_are_exact() {
    let cartridge = cartridge();
    let cpu = Cpu::new();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    let mut pending = PendingSound::prepare(&cpu, &memory, &cartridge, 0).unwrap();
    pending.memory.ram[17] = 0x85;
    pending.writes.0[1] = Some((WORK_RAM_MASK, 0x93));
    pending.psg_writes.0[1] = Some(0x82);
    pending.opn2_writes.0[1] = Some((3, 0xb6));
    let schedule = SoundSchedule {
        pending: Some(pending),
        ..SoundSchedule::default()
    };
    let saved = save(&schedule, &cpu);
    let (restored, restored_cpu) = restore(&saved);
    assert_eq!(restored, schedule);
    assert_eq!(restored_cpu, cpu);
    let mut a = restored.pending.unwrap();
    let mut b = schedule.pending.unwrap();
    a.replay = None;
    b.replay = None;
    let mut cpu_a = cpu.clone();
    let mut cpu_b = cpu;
    let mut memory_a = memory.clone();
    let mut memory_b = memory;
    let mut ram_a = vec![0; WORK_RAM_MASK + 1];
    let mut ram_b = ram_a.clone();
    let mut audio_a = Audio::default();
    let mut audio_b = audio_a.clone();
    a.publish(&mut cpu_a, &mut memory_a, &mut ram_a, &mut audio_a);
    b.publish(&mut cpu_b, &mut memory_b, &mut ram_b, &mut audio_b);
    assert_eq!(
        (cpu_a, memory_a, ram_a, audio_a),
        (cpu_b, memory_b, ram_b, audio_b)
    );
}

#[test]
fn malformed_schedule_does_not_mutate_either_paired_target() {
    let cartridge = cartridge();
    let mut cpu = Cpu::new();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    let pending = PendingSound::prepare(&cpu, &memory, &cartridge, 0).unwrap();
    let original = SoundSchedule {
        pending: Some(pending),
        ..SoundSchedule::default()
    };
    let saved = save(&original, &cpu);
    for len in [0, 1, 73, saved.len() - 1] {
        let mut target = original.clone();
        assert!(
            target
                .read_state(&mut cpu, &mut Reader::new(&saved[..len]))
                .is_err()
        );
        assert_eq!(target, original);
        assert_eq!(cpu, Cpu::new());
    }
    let mut bad = saved;
    *bad.last_mut().unwrap() = 2;
    let mut target = original.clone();
    assert!(target.read_state(&mut cpu, &mut Reader::new(&bad)).is_err());
    assert_eq!(target, original);
    let mut faulted = original;
    faulted.fault = Some(SoundFault::ClockOverflow);
    assert!(faulted.write_state(&cpu, &mut Writer::new()).is_err());
}

#[test]
fn rejects_pending_deadline_and_journal_values_before_publication() {
    let cartridge = cartridge();
    let cpu = Cpu::new();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    let pending = PendingSound::prepare(&cpu, &memory, &cartridge, 0).unwrap();
    for bad in 0..4 {
        let mut pending = pending.clone();
        match bad {
            0 => pending.deadline += 1,
            1 => pending.instruction.cycles += 1,
            2 => pending.writes.0[0] = Some((WORK_RAM_MASK + 1, 7)),
            _ => pending.opn2_writes.0[0] = Some((4, 7)),
        }
        let mut w = Writer::new();
        cpu.write_continuation(&BusState::default(), &mut w);
        w.write_bool(true);
        pending.write_state(&cpu, &mut w).unwrap();
        let mut target = SoundSchedule::default();
        let mut target_cpu = cpu.clone();
        assert!(
            target
                .read_state(&mut target_cpu, &mut Reader::new(&w.into_bytes()))
                .is_err()
        );
        assert_eq!(target, SoundSchedule::default());
        assert_eq!(target_cpu, cpu);
    }
}
