use super::schedule::{PendingSound, SoundSchedule};
use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_z80::Cpu;

fn cartridge() -> Cartridge {
    let mut bytes = vec![0; 16];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 0, 8]);
    Cartridge::new(bytes).unwrap()
}

fn memory(program: &[u8]) -> SoundMemory {
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.ram[..program.len()].copy_from_slice(program);
    memory
}

#[test]
fn prepared_ram_overwrites_the_whole_live_image_and_retains_value_isolation() {
    let cartridge = cartridge();
    for (opcode, address, initial) in [
        (0x3a, 0x0100_u16, 0x17),
        (0x32, 0x0100, 0x17),
        (0x32, 0x0100, 0x5a),
        (0x32, 0x3fff, 0x17),
    ] {
        let [low, high] = address.to_le_bytes();
        let mut memory = memory(&[0x3e, 0x5a, opcode, low, high, 0x76]);
        memory.bank = 0x140;
        memory.write(address, initial);
        let mut cpu = Cpu::new();
        let mut audio = Audio::default();
        PendingSound::prepare(&cpu, &memory, &cartridge, 0)
            .unwrap()
            .publish(&mut cpu, &mut memory, &mut [], &mut audio);
        let mut pending = PendingSound::prepare(&cpu, &memory, &cartridge, 105).unwrap();
        let index = usize::from(address) & SOUND_RAM_MASK;
        assert_eq!(memory.ram[index], initial);
        assert_eq!(
            pending.memory.ram[index],
            if opcode == 0x32 { 0x5a } else { initial }
        );
        let prepared = pending.memory.ram[..].to_vec();
        memory.write(0, 0x11);
        memory.write(0x0800, 0x22);
        memory.write(0x3fff, 0x33);
        memory.bank = 0x1ff;
        memory.request(true);
        memory.irq_asserted = true;
        memory.set_reset(opcode == 0x3a);
        pending.publish(&mut cpu, &mut memory, &mut [], &mut audio);
        assert_eq!(&memory.ram[..], prepared);
        assert_eq!(memory.bank, 0x140);
        assert!(memory.requested);
        assert_eq!(memory.reset_asserted, opcode == 0x3a);
        assert_eq!(memory.granted, opcode != 0x3a);
        assert!(memory.irq_asserted);
        assert_eq!(pending.memory.ram[..], prepared);
        memory.write(0x0800, 0x44);
        assert_eq!(pending.memory.ram[0x0800], prepared[0x0800]);
        pending.memory.write(0x0801, 0x55);
        assert_eq!(memory.ram[0x0801], prepared[0x0801]);
        assert_eq!(memory.ram[0x0800], 0x44);
    }
}

#[test]
fn status_refresh_uses_live_ram_sampled_inputs_and_rejects_transactionally() {
    let cartridge = cartridge();
    let cpu = Cpu::new();
    let mut memory = memory(&[0x3a, 0, 0xc0, 0x76]);
    memory.bank = 0x140;
    memory.irq_asserted = true;
    let pending = PendingSound::prepare(&cpu, &memory, &cartridge, 0).unwrap();
    let mut audio = Audio::default();
    audio.write_opn2(0, 0, 0x2a);
    audio.write_opn2(151, 1, 0x90);
    let original_audio = audio.clone();
    memory.write(0x0777, 0xa6);
    memory.bank = 0;
    memory.irq_asserted = false;
    let mut refreshed = pending.clone();
    refreshed
        .refresh_opn2(&cpu, &memory, &cartridge, &audio)
        .unwrap();
    assert_eq!(refreshed.cpu.regs().a, 0x80);
    assert_eq!(refreshed.memory.ram[0x0777], 0xa6);
    assert_eq!(pending.memory.ram[0x0777], 0);
    assert_eq!(refreshed.memory.bank, 0x140);
    assert!(refreshed.memory.irq_asserted);
    assert_eq!(audio, original_audio);
    let mut published_cpu = cpu.clone();
    refreshed.publish(&mut published_cpu, &mut memory, &mut [], &mut audio);
    assert_eq!(memory.ram[0x0777], 0xa6);
    assert_eq!(memory.bank, 0x140);
    assert!(!memory.irq_asserted);

    for failure in 0..3 {
        let mut rejected = pending.clone();
        let mut source = memory.clone();
        match failure {
            0 => source.ram[0] = 0,
            1 => source.ram[..3].copy_from_slice(&[0x3a, 4, 0x40]),
            _ => rejected.deadline += 1,
        }
        let original = rejected.clone();
        let original_source = source.clone();
        let original_audio = audio.clone();
        let error = rejected
            .refresh_opn2(&cpu, &source, &cartridge, &audio)
            .unwrap_err();
        assert!(matches!(
            (failure, error),
            (0, SoundFault::UnstableOpn2Read { pc: 0 })
                | (
                    1,
                    SoundFault::UnsupportedAccess {
                        address: 0x4004,
                        ..
                    }
                )
                | (2, SoundFault::ClockOverflow)
        ));
        assert_eq!(rejected, original);
        assert_eq!(source, original_source);
        assert_eq!(audio, original_audio);
        assert_eq!(cpu, Cpu::new());
    }
}

#[test]
fn decoded_granted_live_and_pending_images_and_ordinary_clones_are_independent() {
    let cartridge = cartridge();
    let cpu = Cpu::new();
    let mut live = memory(&[0x00, 0x76]);
    let pending = PendingSound::prepare(&cpu, &live, &cartridge, 0).unwrap();
    let mut schedule = SoundSchedule {
        pending: Some(pending),
        ..SoundSchedule::default()
    };
    live.write(0x0100, 0xa5);
    live.request(true);
    live.complete_boundary();
    assert!(live.granted && schedule.pending.is_some());
    let mut writer = StateWriter::new();
    live.write_state(&mut writer).unwrap();
    schedule.write_state(&cpu, &mut writer).unwrap();
    let bytes = writer.into_bytes();
    let mut reader = StateReader::new(&bytes);
    let mut decoded_live = SoundMemory::read_state(&mut reader).unwrap();
    let mut decoded_schedule = SoundSchedule::default();
    let mut decoded_cpu = cpu.clone();
    decoded_schedule
        .read_state(&mut decoded_cpu, &mut reader)
        .unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(decoded_live, live);
    assert_eq!(decoded_schedule, schedule);
    assert_eq!(decoded_cpu, cpu);
    assert_eq!(decoded_live.ram[0x0100], 0xa5);
    assert_eq!(
        decoded_schedule.pending.as_ref().unwrap().memory.ram[0x0100],
        0
    );

    let live_clone = live.clone();
    let schedule_clone = schedule.clone();
    decoded_live.write(0x0100, 0x11);
    decoded_schedule
        .pending
        .as_mut()
        .unwrap()
        .memory
        .write(0x0101, 0x22);
    assert_eq!(live, live_clone);
    assert_eq!(schedule, schedule_clone);
    assert_eq!(decoded_live.ram[0x0101], 0);
    assert_eq!(
        decoded_schedule.pending.as_ref().unwrap().memory.ram[0x0100],
        0
    );
    live.write(0x0102, 0x33);
    schedule
        .pending
        .as_mut()
        .unwrap()
        .memory
        .write(0x0103, 0x44);
    assert_eq!(live_clone.ram[0x0102], 0);
    assert_eq!(
        schedule_clone.pending.as_ref().unwrap().memory.ram[0x0103],
        0
    );
    assert_eq!(decoded_live.ram[0x0102], 0);
    assert_eq!(
        decoded_schedule.pending.as_ref().unwrap().memory.ram[0x0103],
        0
    );
}
