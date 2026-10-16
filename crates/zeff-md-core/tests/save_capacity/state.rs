use super::fixture::{Rom, config, expected, run_to_phase};
use zeff_md_core::{Emulator, Region};

fn fixture(size: usize, region: Region, concurrent: bool) -> Emulator {
    let mut rom = Rom::new(size);
    rom.enable();
    rom.psg();
    rom.byte(0x200001, 0x47);
    rom.byte(0x200000 + size as u32 * 2 - 1, 0xb3);
    let mut machine = Emulator::new(rom.finish(), config(region, concurrent)).unwrap();
    machine.load_save_ram(&expected(size, 0x93)).unwrap();
    machine.reset_and_begin_audio_trace(8).unwrap();
    run_to_phase(&mut machine, 3);
    machine
}

fn unchanged(machine: &Emulator, before: &Emulator) {
    assert_eq!(machine, before);
    let mut actual = machine.clone();
    let mut expected = before.clone();
    assert_eq!(actual.finish_audio_trace(), expected.finish_audio_trace());
}

#[test]
fn save_and_state_rejections_preserve_complete_machine_and_active_trace() {
    for size in [512, 8192, 32768] {
        for concurrent in [false, true] {
            let mut machine = fixture(size, Region::Pal, concurrent);
            let before = machine.clone();
            let saved = machine.encode_state().unwrap();
            assert_eq!(&saved[8..12], &15_u32.to_le_bytes());
            let section = saved.len() - size - 6;
            let presence = section + 4;
            let enabled = presence + 1;
            assert_eq!(
                &saved[section..presence],
                &((size + 2) as u32).to_le_bytes()
            );
            assert_eq!(&saved[presence..enabled + 1], &[1, 1]);
            for length in [
                0,
                size - 1,
                size + 1,
                if size == 8192 { 32768 } else { 8192 },
            ] {
                assert!(machine.load_save_ram(&vec![0; length]).is_err());
                unchanged(&machine, &before);
            }
            let mut malformed = Vec::new();
            for cut in [0, 12, section, presence, enabled, saved.len() - 1] {
                malformed.push(saved[..cut].to_vec());
            }
            for (offset, value) in [(presence, 0), (presence, 2), (enabled, 2)] {
                let mut bytes = saved.clone();
                bytes[offset] = value;
                malformed.push(bytes);
            }
            if size != 512 {
                let mut disabled = saved.clone();
                disabled[enabled] = 0;
                malformed.push(disabled);
            }
            for length in [size + 1, size + 3] {
                let mut bytes = saved.clone();
                bytes[section..presence].copy_from_slice(&(length as u32).to_le_bytes());
                malformed.push(bytes);
            }
            let mut trailing = saved.clone();
            trailing.push(0);
            malformed.push(trailing);
            let mut extra_section = saved.clone();
            extra_section.push(0);
            extra_section[section..presence].copy_from_slice(&((size + 3) as u32).to_le_bytes());
            malformed.push(extra_section);
            let mut old = saved[..section].to_vec();
            old[8..12].copy_from_slice(&14_u32.to_le_bytes());
            malformed.push(old);
            let other_size = if size == 8192 { 32768 } else { 8192 };
            let other = fixture(other_size, Region::Pal, concurrent);
            let foreign = other.encode_state().unwrap();
            malformed.push(foreign.clone());
            let mut forged_identity = foreign;
            forged_identity[12..44].copy_from_slice(&saved[12..44]);
            malformed.push(forged_identity);
            for bytes in malformed {
                assert!(
                    machine.decode_state(&bytes).is_err(),
                    "size={size} bytes={}",
                    bytes.len()
                );
                unchanged(&machine, &before);
            }
            assert!(
                machine
                    .clone()
                    .finish_audio_trace()
                    .unwrap()
                    .trace
                    .events
                    .len()
                    >= 3
            );
        }
    }
}

#[test]
fn nontrivial_state_roundtrip_restores_cells_frame_and_future_active_audio() {
    for size in [512, 8192, 32768] {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent in [false, true] {
                let mut machine = fixture(size, region, concurrent);
                let saved = machine.encode_state().unwrap();
                let cells = machine.dump_save_ram();
                let mut restored = machine.clone();
                restored.load_save_ram(&vec![0x11; size]).unwrap();
                restored.decode_state(&saved).unwrap();
                machine.decode_state(&saved).unwrap();
                assert_eq!(restored.dump_save_ram(), cells);
                assert_eq!(restored.encode_state().unwrap(), saved);
                machine.drain_audio_samples();
                restored.drain_audio_samples();
                for _ in 0..3000 {
                    assert_eq!(machine.step_instruction(), restored.step_instruction());
                }
                assert_eq!(
                    machine.encode_state().unwrap(),
                    restored.encode_state().unwrap()
                );
                assert_eq!(machine.framebuffer(), restored.framebuffer());
                let audio = machine.drain_audio_samples();
                assert!(!audio.is_empty());
                assert!(audio.iter().any(|sample| *sample != 0.0));
                assert_eq!(audio, restored.drain_audio_samples());
                restored.load_save_ram(&vec![0x55; size]).unwrap();
                assert_ne!(restored.dump_save_ram(), machine.dump_save_ram());
                assert_eq!(restored.rom_bytes(), machine.rom_bytes());
            }
        }
    }
}

#[test]
fn legacy_512_disabled_mapping_is_still_a_valid_state() {
    let mut machine = fixture(512, Region::NtscUs, true);
    let cells = machine.dump_save_ram();
    let mut disabled = machine.encode_state().unwrap();
    let enabled = disabled.len() - 513;
    disabled[enabled] = 0;
    machine.decode_state(&disabled).unwrap();
    assert_eq!(machine.encode_state().unwrap(), disabled);
    assert_eq!(machine.dump_save_ram(), cells);
    assert_eq!(machine.peek(0x200001), None);
    assert_eq!(machine.peek(0x2003ff), None);
}
