use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn write_byte(words: &mut Vec<u16>, address: u32, value: u8) {
    words.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

pub(super) fn machine(region: Region, stopped: bool) -> Emulator {
    let mut sound = vec![0x21, 0x00, 0x07];
    for (port, value) in [
        (0x4002_u16, 0xb6),
        (0x4003, 0x80),
        (0x4000, 0x2a),
        (0x4001, 0xff),
        (0x4000, 0x2b),
        (0x4001, 0x80),
    ] {
        sound.extend([0x3e, value, 0x32, port as u8, (port >> 8) as u8]);
    }
    sound.extend([0x3a, 0x00, 0x40, 0x32, 0x10, 0x07, 0x34, 0x18, 0xf7]);
    let mut words = Vec::new();
    write_byte(&mut words, 0xa11100, 1);
    write_byte(&mut words, 0xa11200, 1);
    for (index, value) in sound.into_iter().enumerate() {
        write_byte(&mut words, 0xa00000 + index as u32, value);
    }
    write_byte(&mut words, 0xa11100, 0);
    words.extend(if stopped {
        [0x4e72, 0x2700]
    } else {
        [0x60fe, 0x4e71]
    });
    let mut rom = vec![0; 0x200 + words.len() * 2];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in words.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .unwrap()
}

fn replace_cpu_states(machine: &mut Emulator) {
    let mut writer = StateWriter::new();
    machine.cpu.write_boundary_state(&mut writer).unwrap();
    machine.cpu = PrefetchCpu::default();
    machine
        .cpu
        .read_boundary_state(&mut StateReader::new(&writer.into_bytes()))
        .unwrap();

    let mut writer = StateWriter::new();
    machine
        .sound_cpu
        .write_continuation(&machine.sound_schedule.state, &mut writer);
    machine.sound_cpu = zeff_z80::Cpu::new();
    machine.sound_schedule.state = zeff_z80::BusState::default();
    machine
        .sound_cpu
        .read_continuation(
            &mut machine.sound_schedule.state,
            &mut StateReader::new(&writer.into_bytes()),
        )
        .unwrap();
    if let Some(pending) = &mut machine.sound_schedule.pending {
        let mut writer = StateWriter::new();
        pending.cpu.write_continuation(&pending.state, &mut writer);
        pending.cpu = zeff_z80::Cpu::new();
        pending.state = zeff_z80::BusState::default();
        pending
            .cpu
            .read_continuation(
                &mut pending.state,
                &mut StateReader::new(&writer.into_bytes()),
            )
            .unwrap();
    }
}

#[test]
fn cpu_codecs_preserve_live_md_pending_publication_and_audio() {
    for region in [Region::NtscUs, Region::Pal] {
        for stopped in [false, true] {
            let mut source = machine(region, stopped);
            for _ in 0..20_000 {
                if source.sound_cpu.cycles() >= 400 {
                    break;
                }
                source.step_instruction().unwrap();
            }
            assert!(source.sound_cpu.cycles() >= 400);
            assert_eq!(source.cpu.stopped(), stopped);
            assert!(source.opn2_snapshot().dac_enabled);
            source.drain_audio_samples();
            let mut saw_source = false;
            let mut saw_candidate = false;
            let mut samples = 0;
            let mut audible = false;
            for _ in 0..32 {
                saw_source |= source.sound_schedule.state.pending();
                saw_candidate |= source
                    .sound_schedule
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.state.pending());
                let mut target = source.clone();
                replace_cpu_states(&mut target);
                assert_eq!(source, target);
                for _ in 0..16 {
                    assert_eq!(
                        source.step_instruction().unwrap(),
                        target.step_instruction().unwrap()
                    );
                    assert_eq!(source, target);
                }
                let expected = source.drain_audio_samples();
                assert_eq!(expected, target.drain_audio_samples());
                for frame in expected.as_chunks::<2>().0 {
                    assert_eq!(frame[1], 0.0);
                    audible |= frame[0] != 0.0;
                }
                samples += expected.len();
            }
            assert!(saw_source && saw_candidate && samples > 0 && audible);
        }
    }
}

#[test]
fn malformed_cpu_sections_preserve_the_live_md_machine() {
    let mut machine = machine(Region::NtscUs, false);
    for _ in 0..300 {
        machine.step_instruction().unwrap();
    }
    let before = machine.clone();
    assert!(
        machine
            .cpu
            .read_boundary_state(&mut StateReader::new(&[2]))
            .is_err()
    );
    assert_eq!(machine, before);
    assert!(
        machine
            .sound_cpu
            .read_continuation(
                &mut machine.sound_schedule.state,
                &mut StateReader::new(&[2, 0])
            )
            .is_err()
    );
    assert_eq!(machine, before);
}
