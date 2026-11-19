use zeff_md_core::{Emulator, LoadConfig, Region};

fn store(code: &mut Vec<u16>, address: u32, value: u16, word: bool) {
    code.extend([
        if word { 0x33fc } else { 0x13fc },
        value,
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn fixture(region: Region, concurrent_sound: bool, old: u8, v30: bool) -> (Emulator, u32) {
    let mut code = Vec::new();
    for value in [
        if v30 { 0x811c } else { 0x8114 },
        0x8c00 | u16::from(old),
        0x8f02,
        0x8701,
        0xc002,
        0,
    ] {
        store(&mut code, 0xc00004, value, true);
    }
    store(&mut code, 0xc00000, 0x000e, true);
    code.extend([0x4e71; 64]);
    let change_pc = 0x100 + code.len() as u32 * 2;
    store(&mut code, 0xc00004, 0x8c01, true);
    store(&mut code, 0xff0200, 0xcafe, true);
    for value in [0x85, 0x06, 0x90] {
        store(&mut code, 0xc00011, value, false);
    }
    code.extend([0x60fe, 0x4e71]);
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in code.into_iter().enumerate() {
        rom[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap();
    machine.set_sample_rate(48_000);
    (machine, change_pc)
}

fn observations(machine: &mut Emulator) -> (Vec<u8>, Vec<u8>, Vec<u32>, u64) {
    machine.step_field().unwrap();
    machine.step_field().unwrap();
    assert_eq!(
        (machine.peek(0xff0200), machine.peek(0xff0201)),
        (Some(0xca), Some(0xfe))
    );
    assert_eq!(machine.vdp().registers().registers()[12], Some(1));
    let height = if machine.region() == Region::Pal
        && machine.vdp().registers().registers()[1].unwrap() & 8 != 0
    {
        240
    } else {
        224
    };
    assert_eq!(machine.framebuffer_dimensions(), (320, height));
    assert!(machine.video_error().is_none());
    assert!(machine.frame_count() >= 2);
    assert!(
        machine
            .framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [255, 0, 0, 255])
    );
    let samples: Vec<_> = machine
        .drain_audio_samples()
        .iter()
        .map(|sample| sample.to_bits())
        .collect();
    assert!(samples.iter().any(|bits| *bits != 0));
    (
        machine.encode_frontend_state().unwrap(),
        machine.framebuffer().to_vec(),
        samples,
        machine.nominal_frame_duration_ns(),
    )
}

pub fn run() -> u64 {
    let mut signature = 0xcbf29ce484222325u64;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for old in [0, 0x81] {
                for v30 in [false, true] {
                    if v30 && region != Region::Pal {
                        continue;
                    }
                    let (mut source, pc) = fixture(region, concurrent, old, v30);
                    for _ in 0..100 {
                        if source.registers().pc == pc {
                            break;
                        }
                        source.step_instruction().unwrap();
                    }
                    assert_eq!(source.registers().pc, pc);
                    assert!(source.vdp().fifo().is_empty());
                    let before = source.encode_frontend_state().unwrap();
                    assert_eq!(u32::from_le_bytes(before[8..12].try_into().unwrap()), 1);
                    source.step_instruction().unwrap();
                    let after = source.encode_frontend_state().unwrap();
                    let expected = observations(&mut source);
                    for saved in [&before, &after] {
                        let (mut target, _) = fixture(region, concurrent, old, v30);
                        target.decode_frontend_state(saved).unwrap();
                        if saved == &before {
                            target.step_instruction().unwrap();
                        }
                        assert_eq!(observations(&mut target), expected);
                    }
                    let (mut rewound, _) = fixture(region, concurrent, old, v30);
                    rewound.decode_frontend_state(&expected.0).unwrap();
                    let checkpoint = rewound.encode_frontend_state().unwrap();
                    let next = observations(&mut rewound);
                    rewound.decode_frontend_state(&checkpoint).unwrap();
                    assert_eq!(observations(&mut rewound), next);
                    for byte in expected.0 {
                        signature = (signature ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                    }
                }
            }
        }
    }
    signature
}
