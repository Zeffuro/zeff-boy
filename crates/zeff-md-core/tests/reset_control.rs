use zeff_emu_common::time::MachineTiming;
use zeff_md_core::{Emulator, LoadConfig, Region};

const REQUEST: u32 = 0xa11100;
const RESET: u32 = 0xa11200;
const MARKER: u32 = 0xff0200;
const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];

fn store(code: &mut Vec<u16>, address: u32, value: u16, word: bool) {
    code.extend_from_slice(&[
        if word { 0x33fc } else { 0x13fc },
        value,
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn program() -> (Vec<u16>, u32) {
    let mut code = Vec::new();
    store(&mut code, REQUEST, 0x100, true);
    store(&mut code, RESET, 0x100, true);
    code.extend_from_slice(&[0x0839, 0, 0x00a1, 0x1100, 0x66f6]);
    store(&mut code, 0xa00000, 0x76, false);
    store(&mut code, 0xa00100, 0xa5, false);
    let clr = 0x100 + code.len() as u32 * 2;
    code.extend_from_slice(&[0x4279, 0x00a1, 0x1200]);
    code.extend_from_slice(&[0x4e71; 5]);
    store(&mut code, RESET, 0x100, true);
    code.extend_from_slice(&[0x4e71; 5]);
    code.extend_from_slice(&[0x1639, 0x00a0, 0x0100]);
    code.extend_from_slice(&[0x4279, 0x00a1, 0x1100]);
    store(&mut code, MARKER, 0xcafe, true);
    store(&mut code, 0xc00004, 0x8a7f, true);
    store(&mut code, 0xc00011, 0x80, false);
    store(&mut code, 0xc00011, 0x10, false);
    store(&mut code, 0xc00011, 0x90, false);
    code.extend_from_slice(&[0x1439, 0x00a1, 0x0003, 0x60fe, 0x4e71]);
    (code, clr)
}

fn machine(code: &[u16], region: Region, concurrent_sound: bool) -> Emulator {
    let mut rom = vec![0; 0x1000];
    rom[..8].copy_from_slice(&[0, 255, 255, 252, 0, 0, 1, 0]);
    for (index, word) in code.iter().enumerate() {
        rom[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

fn before_clr(machine: &mut Emulator, pc: u32) {
    for _ in 0..128 {
        if machine.registers().pc == pc {
            assert!(machine.sound_bus_requested());
            assert!(machine.sound_bus_granted());
            assert!(!machine.sound_reset_asserted());
            assert_eq!(machine.peek(0xa00100), Some(0xa5));
            return;
        }
        machine.step_instruction().unwrap();
    }
    panic!("reset sequence did not reach CLR");
}

fn finish(machine: &mut Emulator) {
    for _ in 0..64 {
        machine.step_instruction().unwrap();
        if machine.peek(MARKER) == Some(0xca) {
            break;
        }
    }
    assert_eq!(machine.peek(MARKER + 1), Some(0xfe));
    assert_eq!(machine.registers().d[3], 0xa5);
    assert!(!machine.sound_bus_requested());
    assert!(!machine.sound_reset_asserted());
    for _ in 0..10 {
        machine.step_instruction().unwrap();
    }
    machine.step_field().unwrap();
}

#[test]
fn actual_clr_reset_sequence_finishes_in_every_region_and_scheduler() {
    let (code, clr) = program();
    for region in REGIONS {
        for concurrent in [false, true] {
            let mut machine = machine(&code, region, concurrent);
            before_clr(&mut machine, clr);
            let start = machine.timing_snapshot().now().get();
            let result = machine.step_instruction().unwrap();
            assert_eq!(result.elapsed.get(), 140);
            assert_eq!(machine.timing_snapshot().now().get() - start, 140);
            assert!(machine.sound_reset_asserted());
            assert!(machine.sound_bus_requested());
            assert!(!machine.sound_bus_granted());
            finish(&mut machine);
            assert!(
                machine
                    .drain_audio_samples()
                    .iter()
                    .any(|sample| *sample != 0.0)
            );
        }
    }
}

#[test]
fn actual_reset_read_returns_next_prefetched_word_to_move_destination() {
    for region in REGIONS {
        for concurrent in [false, true] {
            let code = [0x3039, 0x00a1, 0x1200, 0x7245, 0x4e71, 0x60fe, 0x4e71];
            let mut machine = machine(&code, region, concurrent);
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[0], 0x7245);
            assert!(machine.sound_reset_asserted());
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[1], 0x45);
        }
    }
}

#[test]
fn actual_reset_sequence_continues_exactly_after_raw_and_frontend_restore() {
    let (code, clr) = program();
    for region in REGIONS {
        for concurrent in [false, true] {
            let mut source = machine(&code, region, concurrent);
            source.set_sample_rate(48_000);
            before_clr(&mut source, clr);
            for frontend in [false, true] {
                let state = if frontend {
                    source.encode_frontend_state().unwrap()
                } else {
                    source.encode_state().unwrap()
                };
                let mut expected = source.clone();
                let mut restored = source.clone();
                restored.step_instruction().unwrap();
                if frontend {
                    restored.decode_frontend_state(&state).unwrap();
                } else {
                    restored.decode_state(&state).unwrap();
                    expected.decode_state(&state).unwrap();
                }
                finish(&mut expected);
                finish(&mut restored);
                assert_eq!(
                    restored.encode_state().unwrap(),
                    expected.encode_state().unwrap()
                );
                assert_eq!(
                    restored.drain_audio_samples(),
                    expected.drain_audio_samples()
                );
                assert_eq!(restored.framebuffer(), expected.framebuffer());
            }
        }
    }
}
