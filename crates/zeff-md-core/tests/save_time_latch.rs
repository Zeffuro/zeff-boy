#[path = "support/state_layout.rs"]
mod state_layout;

use zeff_emu_common::rewind::RewindBuffer;
use zeff_emu_common::time::Reset;
use zeff_md_core::{Emulator, LoadConfig, Region};

const PRODUCTS: [&[u8]; 2] = [b"GM MK-1354 -00", b"GM G-5543  -00"];
const CELLS: usize = 8192;
const COPY_LONGS: usize = 0x12e;

fn fixture(product: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x69; 0x300000];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x180..0x18e].copy_from_slice(product);
    bytes[0x1b0..0x1bc]
        .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 0x3f, 0xff]);
    let mut program = Vec::<u16>::new();
    for (index, value) in [1, 2, 3, 4, 5, 0, 0xff, 2].into_iter().enumerate() {
        let address = 0xa130f0 + index as u32 * 2;
        program.extend([
            0x13fc,
            value,
            (address >> 16) as u16,
            address as u16 | 1,
            0x3c3c,
            10 + index as u16,
        ]);
    }
    program.extend([0x33fc, 0xab03, 0x00a1, 0x30f0, 0x3c3c, 1]);
    program.extend([
        0x41f9,
        0x0020,
        0x0800,
        0x43f9,
        0x0020,
        0x0000,
        0x323c,
        COPY_LONGS as u16 - 1,
    ]);
    program.extend([0x22d8, 0x51c9, 0xfffc, 0x3c3c, 2]);
    program.extend([0x13fc, 2, 0x00a1, 0x30f1, 0x3c3c, 4]);
    for value in [0x85, 6, 0x90] {
        program.extend([0x13fc, value, 0x00c0, 0x0011]);
    }
    program.extend([0x3c3c, 3, 0x60fe]);
    for (index, word) in program.into_iter().enumerate() {
        bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    bytes
}

fn machine(product: &[u8], region: Region, concurrent_sound: bool) -> Emulator {
    let mut machine = Emulator::new(
        fixture(product),
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap();
    machine.set_sample_rate(48_000);
    machine
}

fn cells() -> Vec<u8> {
    let mut cells = vec![0xff; CELLS];
    for (index, value) in cells[1024..1024 + COPY_LONGS * 2].iter_mut().enumerate() {
        *value = (index as u8).wrapping_mul(37) ^ (index >> 8) as u8;
    }
    cells
}

fn phase(machine: &mut Emulator, expected: u32) {
    for _ in 0..5000 {
        if machine.registers().d[6] == expected {
            return;
        }
        machine.step_instruction().unwrap();
    }
    panic!("phase {expected} not reached");
}

#[cfg_attr(test, test)]
pub fn aliases_latch_bit_zero_and_backup_copy_writes_under_control_three() {
    for product in PRODUCTS {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent in [false, true] {
                let mut machine = machine(product, region, concurrent);
                let initial = cells();
                machine.load_save_ram(&initial).unwrap();
                assert_eq!(machine.peek(0x200001), Some(0x69));
                for (index, enabled) in [true, false, true, false, true, false, true, false]
                    .into_iter()
                    .enumerate()
                {
                    phase(&mut machine, 10 + index as u32);
                    assert_eq!(
                        machine.peek(0x200001),
                        Some(if enabled { 0xff } else { 0x69 })
                    );
                    assert_eq!(machine.dump_save_ram(), Some(initial.clone()));
                }
                phase(&mut machine, 1);
                assert_eq!(machine.peek(0x200001), Some(0xff));
                phase(&mut machine, 2);
                let mut expected = initial.clone();
                expected[..COPY_LONGS * 2].copy_from_slice(&initial[1024..1024 + COPY_LONGS * 2]);
                assert_eq!(machine.dump_save_ram(), Some(expected.clone()));
                for (index, value) in expected.iter().enumerate() {
                    assert_eq!(machine.peek(0x300001 + index as u32 * 2), Some(*value));
                    assert_eq!(machine.peek(0x200000 + index as u32 * 2), Some(0xff));
                }
                phase(&mut machine, 4);
                assert_eq!(machine.peek(0x200001), Some(0x69));
                assert_eq!(machine.dump_save_ram(), Some(expected));
            }
        }
    }
}

#[cfg_attr(test, test)]
pub fn rewind_reset_and_transactional_load_preserve_mapping_cells_and_future_audio() {
    for product in PRODUCTS {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent in [false, true] {
                let mut actual = machine(product, region, concurrent);
                actual.load_save_ram(&cells()).unwrap();
                phase(&mut actual, 1);
                let enabled = actual.encode_state().unwrap();
                assert_eq!(&enabled[8..12], &1_u32.to_le_bytes());
                let envelope = actual.encode_frontend_state().unwrap();
                let mut rewind = RewindBuffer::new(1, 1);
                rewind.advance_frames(1);
                rewind.push(&envelope, actual.framebuffer());
                let before = actual.clone();
                assert!(actual.load_save_ram(&vec![0; CELLS - 1]).is_err());
                assert_eq!(actual, before);
                phase(&mut actual, 3);
                let changed = actual.dump_save_ram();
                assert_ne!(changed, before.dump_save_ram());
                actual.reset_cpu().unwrap();
                assert_eq!(actual.dump_save_ram(), changed);
                actual.reset();
                assert_eq!(actual.dump_save_ram(), changed);
                assert_eq!(actual.peek(0x200001), Some(0x69));
                let snapshot = rewind.pop().unwrap();
                actual.decode_frontend_state(&snapshot.state_bytes).unwrap();
                assert_eq!(actual.framebuffer(), snapshot.framebuffer);
                assert_eq!(actual.encode_state().unwrap(), enabled);
                let mut restored = machine(product, region, concurrent);
                restored.decode_frontend_state(&envelope).unwrap();
                let mut raw_restored = machine(product, region, concurrent);
                raw_restored.decode_state(&enabled).unwrap();
                let mut raw_control = before.clone();
                raw_control.decode_state(&enabled).unwrap();
                let mut malformed = enabled.clone();
                let flag = state_layout::save_range(&malformed).start + 1;
                malformed[flag] = 2;
                let unchanged = actual.clone();
                assert!(actual.decode_state(&malformed).is_err());
                assert_eq!(actual, unchanged);
                actual.drain_audio_samples();
                restored.drain_audio_samples();
                for _ in 0..2 {
                    actual.step_field().unwrap();
                    restored.step_field().unwrap();
                    raw_restored.step_field().unwrap();
                    raw_control.step_field().unwrap();
                    assert_eq!(
                        actual.encode_state().unwrap(),
                        restored.encode_state().unwrap()
                    );
                    assert_eq!(actual.framebuffer(), restored.framebuffer());
                    assert_eq!(
                        actual.encode_state().unwrap(),
                        raw_restored.encode_state().unwrap()
                    );
                    assert_eq!(
                        raw_control.encode_state().unwrap(),
                        raw_restored.encode_state().unwrap()
                    );
                    let pcm = actual.drain_audio_samples();
                    assert!(!pcm.is_empty() && pcm.iter().any(|sample| *sample != 0.0));
                    assert_eq!(pcm, restored.drain_audio_samples());
                    assert_eq!(
                        raw_control.drain_audio_samples(),
                        raw_restored.drain_audio_samples()
                    );
                }
            }
        }
    }
}

#[cfg_attr(test, test)]
pub fn unrelated_boards_and_unqualified_control_lanes_remain_guarded() {
    for (product, address, value) in [
        (PRODUCTS[0], 0xa130f0, 1),
        (PRODUCTS[1], 0xa130ef, 1),
        (b"GM MK-1307 -00".as_slice(), 0xa130f3, 1),
        (b"GM MK-1307 -00".as_slice(), 0xa130f1, 3),
    ] {
        let mut rom = fixture(product);
        rom[0x202..0x204].copy_from_slice(&u16::to_be_bytes(value));
        rom[0x204..0x208].copy_from_slice(&u32::to_be_bytes(address));
        let mut machine = Emulator::new(rom, LoadConfig::default()).unwrap();
        let cells = machine.dump_save_ram();
        assert!(machine.step_instruction().is_err());
        assert_eq!(machine.dump_save_ram(), cells);
        assert_eq!(machine.peek(0x200001), Some(0x69));
    }
}
