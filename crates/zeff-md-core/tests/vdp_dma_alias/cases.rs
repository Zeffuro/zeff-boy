use zeff_emu_common::time::MachineTiming;
use zeff_md_core::{Emulator, LoadConfig, Region};

fn fixture(region: Region, concurrent_sound: bool, code: u8) -> (Emulator, u32) {
    let mut words = Vec::new();
    for value in [0x8f02, 0x8114, 0x4000, 0] {
        words.extend([0x33fc, value, 0x00c0, 4]);
    }
    for value in [0x1234, 0x5678] {
        words.extend([0x33fc, value, 0x00c0, 0]);
    }
    words.extend([0x4e71; 32]);
    for value in [0x8f01, 0x9304, 0x9400, 0x9500, 0x9600, 0x97c0] {
        words.extend([0x33fc, value, 0x00c0, 4]);
    }
    words.extend([0x207c, 0x00c0, 4, 0x203c]);
    words.push(0x0100 | (u16::from(code & 3) << 14));
    words.push(u16::from(code & 0x3c) << 2);
    let command_pc = 0x200 + words.len() as u32 * 2;
    words.extend([0x2080, 0x4e71, 0x60fc]);
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    (
        Emulator::new(
            rom,
            LoadConfig {
                region,
                concurrent_sound,
            },
        )
        .unwrap(),
        command_pc,
    )
}

fn finish(machine: &mut Emulator, code: u8) {
    for _ in 0..40 {
        machine.step_instruction().unwrap();
    }
    assert_eq!(
        &machine.vdp().memory().vram()[0x100..0x104],
        &[0x12, 0x34, 0x56, 0x78]
    );
    assert_eq!(machine.vdp().command().code, code & !0x20);
    assert_eq!(machine.vdp().command().address, 0x104);
    assert_eq!(machine.vdp().registers().registers()[19], Some(0));
    assert_eq!(machine.vdp().registers().registers()[21], Some(4));
    assert!(
        machine
            .vdp()
            .memory()
            .known_cram()
            .unwrap()
            .iter()
            .all(|word| *word == 0)
    );
    assert!(machine.vdp().memory().vsram().iter().all(|word| *word == 0));
}

pub fn guest_copy_aliases_and_state_continuation() -> u64 {
    let mut signature = 0xcbf29ce484222325u64;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for code in [0x30, 0x31, 0x33, 0x35, 0x3f] {
                let (mut machine, pc) = fixture(region, concurrent, code);
                for _ in 0..64 {
                    if machine.registers().pc == pc {
                        break;
                    }
                    machine.step_instruction().unwrap();
                }
                assert_eq!(machine.registers().pc, pc);
                for _ in 0..4 {
                    let bytes = machine.encode_state().unwrap();
                    let mut expected = machine.clone();
                    let mut restored = machine.clone();
                    restored.step_instruction().unwrap();
                    restored.decode_state(&bytes).unwrap();
                    finish(&mut expected, code);
                    finish(&mut restored, code);
                    assert_eq!(
                        expected.encode_state().unwrap(),
                        restored.encode_state().unwrap()
                    );
                    assert_eq!(
                        expected.timing_snapshot().now(),
                        restored.timing_snapshot().now()
                    );
                    for byte in expected.encode_state().unwrap() {
                        signature = (signature ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                    }
                    machine.step_instruction().unwrap();
                }
            }
        }
    }
    signature
}
