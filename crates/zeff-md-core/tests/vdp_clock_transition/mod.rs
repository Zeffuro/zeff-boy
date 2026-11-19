use zeff_emu_common::time::MachineTiming;
use zeff_md_core::{Emulator, LoadConfig, Region};

const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];
const MARKER: u32 = 0xff0200;

fn store(code: &mut Vec<u16>, address: u32, value: u16, word: bool) {
    code.extend_from_slice(&[
        if word { 0x33fc } else { 0x13fc },
        value,
        (address >> 16) as u16,
        address as u16,
    ]);
}

pub(super) fn fixture(region: Region, concurrent_sound: bool, before: u8) -> (Emulator, u32) {
    let mut words = Vec::new();
    for value in [0x8c00 | u16::from(before), 0x8f02, 0x8144] {
        store(&mut words, 0xc00004, value, true);
    }
    for value in [0x4048, 0x0010] {
        store(&mut words, 0xc00004, value, true);
    }
    for value in [0x0123, 0x0234, 0x0345, 0] {
        store(&mut words, 0xc00000, value, true);
    }
    let change_pc = 0x100 + words.len() as u32 * 2;
    store(
        &mut words,
        0xc00004,
        0x8c00 | u16::from(before ^ 0x81),
        true,
    );
    store(&mut words, MARKER, 0xcafe, true);
    for (address, value) in [(0xc00004, 0xc000), (0xc00004, 0), (0xc00000, 0x0eee)] {
        store(&mut words, address, value, true);
    }
    for value in [0x80, 0x10, 0x90] {
        store(&mut words, 0xc00011, value, false);
    }
    words.extend([0x60fe, 0x4e71]);
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in words.into_iter().enumerate() {
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

pub(super) fn before_change(machine: &mut Emulator, pc: u32) {
    for _ in 0..32 {
        if machine.registers().pc == pc {
            assert!(!machine.vdp().fifo().is_empty());
            return;
        }
        machine.step_instruction().unwrap();
    }
    panic!("guest did not reach width request");
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |value, byte| {
        (value ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn future(mut machine: Emulator) -> (Vec<u8>, Vec<u8>, Vec<u32>, (usize, usize)) {
    for _ in 0..12 {
        machine.step_instruction().unwrap();
    }
    assert_eq!(
        (machine.peek(MARKER), machine.peek(MARKER + 1)),
        (Some(0xca), Some(0xfe))
    );
    machine.step_field().unwrap();
    machine.step_field().unwrap();
    assert!(machine.vdp().fifo().is_empty());
    assert_eq!(
        &machine.vdp().memory().vsram()[36..40],
        &[0x123, 0x234, 0x345, 0]
    );
    let samples = machine.drain_audio_samples();
    assert!(samples.iter().any(|value| *value != 0.0));
    let pcm = samples.iter().map(|value| value.to_bits()).collect();
    assert!(
        machine
            .framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3].iter().any(|byte| *byte != 0))
    );
    (
        machine.encode_state().unwrap(),
        machine.framebuffer().to_vec(),
        pcm,
        machine.framebuffer_dimensions(),
    )
}

pub fn actual_guests_publish_width_and_retire_captured_fifo() -> u64 {
    let mut signature = 0xcbf29ce484222325u64;
    for region in REGIONS {
        for concurrent in [false, true] {
            for before in [0, 0x81] {
                let (mut machine, pc) = fixture(region, concurrent, before);
                before_change(&mut machine, pc);
                assert_eq!(machine.vdp().registers().registers()[12], Some(before));
                let start = machine.timing_snapshot().now().get();
                machine.step_instruction().unwrap();
                assert!(machine.timing_snapshot().now().get() > start);
                assert_eq!(
                    machine.vdp().registers().registers()[12],
                    Some(before ^ 0x81)
                );
                let result = future(machine);
                assert_eq!(result.3, (if before == 0 { 320 } else { 256 }, 224));
                let pcm: Vec<_> = result
                    .2
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect();
                signature = signature.wrapping_mul(0x100000001b3)
                    ^ hash(&result.0)
                    ^ hash(&result.1)
                    ^ hash(&pcm);
            }
        }
    }
    signature
}

pub fn raw_and_frontend_restores_preserve_video_audio_and_machine_future() -> u64 {
    let mut signature = 0xcbf29ce484222325u64;
    for region in REGIONS {
        for concurrent in [false, true] {
            for before in [0, 0x81] {
                let (mut machine, pc) = fixture(region, concurrent, before);
                before_change(&mut machine, pc);
                for boundary in 0..3 {
                    for frontend in [false, true] {
                        let bytes = if frontend {
                            machine.encode_frontend_state().unwrap()
                        } else {
                            machine.encode_state().unwrap()
                        };
                        let (mut expected, mut restored) = (machine.clone(), machine.clone());
                        restored.step_instruction().unwrap();
                        if frontend {
                            restored.decode_frontend_state(&bytes).unwrap();
                        } else {
                            assert_eq!(&bytes[8..12], &1_u32.to_le_bytes());
                            expected.decode_state(&bytes).unwrap();
                            restored.decode_state(&bytes).unwrap();
                        }
                        let (a, b) = (future(expected), future(restored));
                        assert_eq!(
                            a, b,
                            "{region:?} concurrent={concurrent} before={before} boundary={boundary} frontend={frontend}"
                        );
                        let pcm: Vec<_> =
                            a.2.iter().flat_map(|value| value.to_le_bytes()).collect();
                        signature = signature.rotate_left(1) ^ hash(&a.0) ^ hash(&a.1) ^ hash(&pcm);
                    }
                    machine.step_instruction().unwrap();
                }
            }
        }
    }
    signature
}
