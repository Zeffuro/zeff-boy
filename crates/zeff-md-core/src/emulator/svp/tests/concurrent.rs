use super::*;
use crate::hardware::sound::SoundFault;
use crate::save_state::MAX_STATE_BYTES;

fn fresh_concurrent(control: bool) -> Emulator {
    let (rom, irom) = images();
    let config = LoadConfig {
        region: Region::NtscUs,
        concurrent_sound: true,
    };
    if control {
        Emulator::new_synthetic_svp_with_control(rom, config, irom).unwrap()
    } else {
        Emulator::new_synthetic_svp(rom, config, irom).unwrap()
    }
}

fn advance(machine: &mut Emulator, ticks: u64) {
    ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    )
    .advance_machine(ticks)
    .unwrap();
}

fn replace_sound(saved: &[u8], sound: &[u8]) -> Vec<u8> {
    let mut reader = StateReader::new(saved);
    reader.set_position(52);
    for _ in 0..2 {
        reader.read_slice(MAX_STATE_BYTES).unwrap();
    }
    let start = reader.position();
    reader.read_slice(MAX_STATE_BYTES).unwrap();
    let mut writer = StateWriter::new();
    writer.write_bytes(&saved[..start]);
    writer.write_vec(sound);
    writer.write_bytes(&saved[reader.position()..]);
    writer.into_bytes()
}

fn malformed_pending(machine: &mut Emulator) {
    let before = machine.clone();
    let saved = machine.encode_state().unwrap();
    for (address, direction) in [
        (0x300000, Direction::Read),
        (0xa15004, Direction::Read),
        (0xa15006, Direction::Write),
    ] {
        let mut schedule = machine.sound_schedule.clone();
        let pending = schedule.pending.as_mut().unwrap();
        let access = pending.bank_access.as_mut().unwrap();
        access.address = address;
        access.direction = direction;
        pending.memory.bank = (address >> 15) as u16;
        let mut writer = StateWriter::new();
        schedule
            .write_state(&machine.sound_cpu, &mut writer)
            .unwrap();
        let malformed = replace_sound(&saved, &writer.into_bytes());
        assert!(machine.decode_state(&malformed).is_err());
        assert!(*machine == before);
    }
    let mut schedule = machine.sound_schedule.clone();
    let pending = schedule.pending.as_mut().unwrap();
    let access = pending.bank_access.take().unwrap();
    assert!(pending.any_bank_access);
    let mut writer = StateWriter::new();
    schedule
        .write_state(&machine.sound_cpu, &mut writer)
        .unwrap();
    let mut sound = writer.into_bytes();
    let mut retained = machine.clone();
    retained
        .decode_state(&replace_sound(&saved, &sound))
        .unwrap();
    assert!(
        retained
            .sound_schedule
            .pending
            .as_ref()
            .unwrap()
            .bank_access
            .is_none()
    );
    let mut tail = StateWriter::new();
    tail.write_bool(true);
    tail.write_u32(access.address);
    tail.write_u8(0);
    tail.write_u8(access.value);
    let tail = tail.into_bytes();
    assert!(sound.ends_with(&tail));
    sound.truncate(sound.len() - tail.len());
    sound.push(0);
    assert!(
        machine
            .decode_state(&replace_sound(&saved, &sound))
            .is_err()
    );
    assert!(*machine == before);
}

fn rejected_accesses(machine: &Emulator) {
    for target in [0x1000, 0x200000, 0x300000, 0xa15004] {
        for writing in [false, true] {
            let mut machine = machine.clone();
            let address = (0x8000 | (target & 0x7fff)) as u16;
            machine.bus.sound.bank = (target >> 15) as u16;
            machine.bus.sound.ram[..4].copy_from_slice(&[
                if writing { 0x32 } else { 0x3a },
                address as u8,
                (address >> 8) as u8,
                0x76,
            ]);
            machine.bus.sound.set_reset(false);
            let dsp = machine.synthetic_svp().unwrap().encode_state().unwrap();
            let main = machine.registers();
            let mut rejected = false;
            for _ in 0..256 {
                let cpu = machine.sound_cpu.clone();
                let sound = machine.bus.sound.clone();
                advance(&mut machine, 1);
                if let Some(fault) = machine.sound_schedule.fault {
                    assert_eq!(
                        fault,
                        SoundFault::UnsupportedAccess {
                            address,
                            direction: if writing {
                                Direction::Write
                            } else {
                                Direction::Read
                            },
                        }
                    );
                    assert_eq!(machine.sound_cpu, cpu);
                    assert_eq!(machine.bus.sound, sound);
                    assert!(machine.sound_schedule.pending.is_none());
                    rejected = true;
                    break;
                }
            }
            assert!(rejected);
            assert_eq!(machine.registers(), main);
            assert_eq!(
                machine.synthetic_svp().unwrap().encode_state().unwrap(),
                dsp
            );
        }
    }
}

pub(super) fn contract() {
    for control in [false, true] {
        let mut machine = fresh_concurrent(control);
        for _ in 0..4 {
            machine.advance_synthetic_svp(true).unwrap();
        }
        machine.step_instruction().unwrap();
        let mut prepared = false;
        for _ in 0..100 {
            if matches!(
                machine.advance_synthetic_svp(true).unwrap(),
                Progress::Prepared { pc: 0x412, .. }
            ) {
                prepared = true;
                break;
            }
        }
        assert!(prepared);
        assert_eq!(machine.synthetic_svp().unwrap().peek_dram(0), 0);
        let dsp = machine.synthetic_svp().unwrap().encode_state().unwrap();
        rejected_accesses(&machine);
        machine.bus.sound.ram[..7].copy_from_slice(&[0x3a, 0, 0x81, 0x32, 0, 1, 0x76]);
        machine.bus.sound.set_reset(false);
        let mut sampled = false;
        for _ in 0..256 {
            advance(&mut machine, 1);
            assert_eq!(machine.sound_schedule.fault, None);
            if let Some(pending) = &machine.sound_schedule.pending
                && pending
                    .bank_access
                    .is_some_and(|access| access.address == 0x100)
            {
                assert_eq!(machine.sound_cpu.regs().a, 0);
                assert_eq!(pending.cpu.regs().a, 0x33);
                sampled = true;
                break;
            }
        }
        assert!(sampled);
        assert_eq!(
            machine.synthetic_svp().unwrap().encode_state().unwrap(),
            dsp
        );
        let saved = machine.encode_state().unwrap();
        assert_eq!(&saved[8..12], &1_u32.to_le_bytes());
        let mut restored = fresh_concurrent(control);
        restored.decode_state(&saved).unwrap();
        assert_eq!(restored.encode_state().unwrap(), saved);
        let mut mismatch = fresh();
        let original = mismatch.encode_state().unwrap();
        assert!(mismatch.decode_state(&saved).is_err());
        assert_eq!(mismatch.encode_state().unwrap(), original);
        let (rom, mut irom) = images();
        irom[0] ^= 1;
        let config = LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        };
        let mut mismatch = if control {
            Emulator::new_synthetic_svp_with_control(rom, config, irom).unwrap()
        } else {
            Emulator::new_synthetic_svp(rom, config, irom).unwrap()
        };
        let original = mismatch.encode_state().unwrap();
        assert!(mismatch.decode_state(&saved).is_err());
        assert_eq!(mismatch.encode_state().unwrap(), original);
        let mut mismatch = fresh_concurrent(!control);
        let original = mismatch.encode_state().unwrap();
        assert!(mismatch.decode_state(&saved).is_err());
        assert_eq!(mismatch.encode_state().unwrap(), original);
        malformed_pending(&mut machine);
        for value in [&mut machine, &mut restored] {
            value.step_instruction().unwrap();
            assert_eq!(&value.work_ram()[0x200..0x202], &[0x77, 0x88]);
            advance(value, 600);
            assert_eq!(value.sound_cpu.regs().a, 0x33);
            assert_eq!(value.bus.sound.ram[0x100], 0x33);
            assert_eq!(value.synthetic_svp().unwrap().encode_state().unwrap(), dsp);
            let main = value.registers();
            let sound_cpu = value.sound_cpu.clone();
            let sound = value.bus.sound.clone();
            assert!(matches!(
                value.advance_synthetic_svp(true),
                Ok(Progress::Committed { pc: 0x412, .. })
            ));
            assert_eq!(value.synthetic_svp().unwrap().peek_dram(0), 0x1235);
            assert_eq!(value.registers(), main);
            assert_eq!(value.sound_cpu, sound_cpu);
            assert_eq!(value.bus.sound, sound);
            assert_eq!(&value.work_ram()[0x200..0x202], &[0x77, 0x88]);
        }
        assert_eq!(
            machine.encode_state().unwrap(),
            restored.encode_state().unwrap()
        );
    }
}
