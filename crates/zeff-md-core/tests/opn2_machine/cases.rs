use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_md_core::{Emulator, LoadConfig, Region};

pub(super) fn write(words: &mut Vec<u16>, address: u32, value: u8) {
    words.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

pub(super) fn chip_write(words: &mut Vec<u16>, register: u8, value: u8) {
    write(words, 0xa04000, register);
    write(words, 0xa04001, value);
}

pub(super) fn machine(words: &[u16], region: Region, concurrent_sound: bool) -> Emulator {
    let mut image = vec![0; 0x200 + words.len() * 2];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in words.iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        image,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

pub(super) fn grant(words: &mut Vec<u16>) {
    write(words, 0xa11100, 1);
    write(words, 0xa11200, 1);
}

pub fn main_cpu_dac_stereo_and_status() {
    for (value, enabled, disabled) in [
        (0, -8416, -256),
        (127, -288, -256),
        (128, 0, 0),
        (129, 64, 0),
        (255, 8128, 0),
    ] {
        for (pan, expected) in [
            (0, [disabled; 2]),
            (0x80, [enabled, disabled]),
            (0x40, [disabled, enabled]),
            (0xc0, [enabled; 2]),
        ] {
            let mut words = Vec::new();
            grant(&mut words);
            write(&mut words, 0xa04002, 0xb6);
            write(&mut words, 0xa04003, pan);
            chip_write(&mut words, 0x2a, value);
            chip_write(&mut words, 0x2b, 0x80);
            words.extend([
                0x1039, 0x00a0, 0x4000, 0x13c0, 0x00ff, 0x0100, 0x60fe, 0x4e71,
            ]);
            for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
                for concurrent in [false, true] {
                    let mut machine = machine(&words, region, concurrent);
                    for _ in 0..10 {
                        machine.step_instruction().unwrap();
                    }
                    assert_eq!(machine.peek(0xff0100).unwrap() & 0x80, 0x80);
                    assert_eq!(machine.opn2_snapshot().dac, value);
                    assert!(machine.opn2_snapshot().dac_enabled);
                    machine.drain_audio_samples();
                    let warm = machine.timing_snapshot().now().get() + region.master_hz() / 1000;
                    while machine.timing_snapshot().now().get() < warm {
                        machine.step_instruction().unwrap();
                    }
                    machine.drain_audio_samples();
                    let end = machine.timing_snapshot().now().get() + region.master_hz() / 200;
                    while machine.timing_snapshot().now().get() < end {
                        machine.step_instruction().unwrap();
                    }
                    let samples = machine.drain_audio_samples();
                    assert!(samples.len() > 400);
                    for frame in samples.as_chunks::<2>().0.iter() {
                        assert_eq!(frame[0], expected[0] as f32 / 32768.0);
                        assert_eq!(frame[1], expected[1] as f32 / 32768.0);
                    }
                    assert_eq!(machine.peek(0xa04000).unwrap() & 0x80, 0);
                }
            }
        }
    }
}

fn sound_program() -> Vec<u8> {
    let mut code = Vec::new();
    let mut emit =
        |port: u16, value: u8| code.extend([0x3e, value, 0x32, port as u8, (port >> 8) as u8]);
    for (register, value) in [
        (0x30, 1),
        (0x34, 1),
        (0x38, 1),
        (0x3c, 1),
        (0x40, 0),
        (0x44, 0),
        (0x48, 0),
        (0x4c, 0),
        (0x50, 31),
        (0x54, 31),
        (0x58, 31),
        (0x5c, 31),
        (0xb0, 7),
        (0xb4, 0xc0),
        (0xa4, 0x22),
        (0xa0, 0x69),
        (0x28, 0xf0),
        (0x24, 0xff),
        (0x25, 3),
        (0x27, 5),
    ] {
        emit(0x4000, register);
        emit(0x4001, value);
    }
    code.extend([0x3a, 0, 0x40, 0xe6, 1, 0x28, 0xf9, 0x32, 0, 7, 0x76]);
    code
}

pub fn uploaded_z80_fm_timer_and_restore() {
    let code = sound_program();
    let mut words = Vec::new();
    grant(&mut words);
    for (offset, &value) in code.iter().enumerate() {
        write(&mut words, 0xa00000 + offset as u32, value);
    }
    write(&mut words, 0xa11100, 0);
    let setup = 3 + code.len();
    words.extend([0x4e72, 0x2700, 0x4e71, 0x4e71]);
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent in [false, true] {
            let mut machine = machine(&words, region, concurrent);
            for _ in 0..setup + 2 {
                machine.step_instruction().unwrap();
            }
            if concurrent {
                while machine.peek(0xa00700) != Some(1) {
                    machine.step_instruction().unwrap();
                }
            } else {
                for _ in 0..100 {
                    machine.step_sound_instruction().unwrap();
                }
            }
            assert_eq!(machine.peek(0xa00700), Some(1));
            assert_eq!(machine.opn2_snapshot().status & 1, 1);
            assert!(machine.sound_fault().is_none());
            machine.drain_audio_samples();
            let mut saved = machine.clone();
            let start = machine.timing_snapshot().now().get();
            let end = start + region.master_hz() / 100;
            while machine.timing_snapshot().now().get() < end {
                if concurrent {
                    assert_eq!(machine.step_instruction(), saved.step_instruction());
                } else {
                    assert_eq!(
                        machine.step_sound_instruction(),
                        saved.step_sound_instruction()
                    );
                }
            }
            assert_eq!(machine, saved);
            let samples = machine.drain_audio_samples();
            assert_eq!(samples, saved.drain_audio_samples());
            assert!(samples.iter().any(|&sample| sample > 0.1));
            assert!(samples.iter().any(|&sample| sample < -0.1));
            assert!(
                samples
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|frame| frame[0] == frame[1])
            );
            machine.reset_sound_cpu();
            assert_eq!(machine.opn2_snapshot(), saved.opn2_snapshot());
            Reset::reset(&mut machine);
            assert_eq!(machine.region(), region);
            assert_eq!(machine.opn2_snapshot().status, 0);
            assert_eq!(machine.opn2_snapshot().output, [0; 6]);
            assert!(machine.drain_audio_samples().is_empty());
        }
    }
}

fn sustain_program(target: u8, pause: u16) -> Vec<u16> {
    fn timed_write(words: &mut Vec<u16>, register: u8, value: u8) {
        chip_write(words, register, value);
        words.extend(std::iter::repeat_n(0x4e71, 64));
    }
    fn wait(words: &mut Vec<u16>, count: u16) {
        words.extend([0x303c, count, 0x4e71, 0x51c8, 0xfffc]);
    }
    let mut words = vec![0x46fc, 0x2700];
    grant(&mut words);
    for slot in [0, 4, 8, 12] {
        for (group, value) in [
            (0x30, 1),
            (0x40, if slot == 0 { 0 } else { 127 }),
            (0x50, 31),
            (0x60, 0),
            (0x70, 0),
            (0x80, 0xf0),
            (0x90, 0),
        ] {
            timed_write(&mut words, group + slot, value);
        }
    }
    for (register, value) in [
        (0xb0, 7),
        (0xb4, 0xc0),
        (0xa4, 0x1c),
        (0xa0, 0),
        (0x28, 0x10),
    ] {
        timed_write(&mut words, register, value);
    }
    wait(&mut words, 600);
    timed_write(&mut words, 0xa4, 0);
    timed_write(&mut words, 0xa0, 0);
    timed_write(&mut words, 0x60, 20);
    wait(&mut words, pause);
    timed_write(&mut words, 0x80, target << 4);
    words.extend([0x60fe, 0x4e71]);
    words
}

pub fn live_sustain_target_must_match_the_decay_level_window() {
    for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
        for concurrent in [false, true] {
            let mut captures = Vec::new();
            for (target, pause) in [(1, 20_000), (15, 20_000), (4, 16_500)] {
                let mut machine = machine(&sustain_program(target, pause), region, concurrent);
                let mut pcm = Vec::new();
                for field in 1..=18 {
                    while machine.timing_snapshot().now().get() < field * region.frame_ticks() {
                        machine.step_instruction().unwrap();
                    }
                    machine.drain_audio_samples_into(&mut pcm);
                }
                assert!(machine.video_error().is_none());
                assert!(pcm.iter().any(|&value| value > 0.1));
                captures.push(pcm);
            }
            assert!(
                captures[0] == captures[1],
                "missed sustain target stopped decay"
            );
            let matched = &captures[2][captures[2].len() - 1024..];
            assert!(matched[0] > 0.01);
            assert!(matched.iter().all(|&value| value == matched[0]));
        }
    }
}

fn rekey_program(attack: u8, release: u8, decay: u8) -> Vec<u16> {
    fn timed_write(words: &mut Vec<u16>, register: u8, value: u8) {
        chip_write(words, register, value);
        words.extend(std::iter::repeat_n(0x4e71, 64));
    }
    let mut words = vec![0x46fc, 0x2700];
    grant(&mut words);
    for slot in [0, 4, 8, 12] {
        for (group, value) in [
            (0x30, 1),
            (0x40, if slot == 0 { 0 } else { 127 }),
            (0x50, 31),
            (0x60, 0),
            (0x70, 0),
            (0x80, 0xf0 | release),
            (0x90, 0),
        ] {
            timed_write(&mut words, group + slot, value);
        }
    }
    for (register, value) in [
        (0xb0, 7),
        (0xb4, 0xc0),
        (0xa4, 0x2f),
        (0xa0, 0),
        (0x28, 0x10),
    ] {
        timed_write(&mut words, register, value);
    }
    words.extend([0x303c, 4000, 0x4e71, 0x51c8, 0xfffc]);
    timed_write(&mut words, 0x50, attack);
    timed_write(&mut words, 0x28, 0);
    timed_write(&mut words, 0x60, decay);
    words.extend([0x303c, 66, 0x4e71, 0x51c8, 0xfffc]);
    timed_write(&mut words, 0x28, 0x10);
    words.extend([0x60fe, 0x4e71]);
    words
}

pub fn zero_level_rekey_uses_decay_while_nonzero_zero_attack_holds() {
    for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
        for concurrent in [false, true] {
            let mut captures = Vec::new();
            for (attack, release, decay) in
                [(0, 0, 20), (1, 0, 20), (31, 0, 20), (0, 15, 20), (0, 15, 0)]
            {
                let mut machine =
                    machine(&rekey_program(attack, release, decay), region, concurrent);
                let mut pcm = Vec::new();
                for field in 1..=18 {
                    while machine.timing_snapshot().now().get() < field * region.frame_ticks() {
                        machine.step_instruction().unwrap();
                    }
                    machine.drain_audio_samples_into(&mut pcm);
                }
                assert!(machine.video_error().is_none());
                assert!(pcm.iter().any(|&value| value > 0.1));
                captures.push(pcm);
            }
            assert!(
                captures[0] == captures[2],
                "zero attack held a zero-level rekey"
            );
            assert!(
                captures[1] == captures[2],
                "slow attack delayed a zero-level rekey"
            );
            assert!(
                captures[3] == captures[4],
                "zero attack changed a nonzero residual"
            );
            let end = &captures[0][captures[0].len() - 1024..];
            assert!(end.iter().all(|&value| value == 0.0));
            let held = &captures[3][captures[3].len() - 1024..];
            assert!(held.iter().any(|&value| value > 0.1));
        }
    }
}

fn channel_three_program(mode: u8, key_mask: u8) -> Vec<u16> {
    fn timed_write(words: &mut Vec<u16>, register: u8, value: u8) {
        chip_write(words, register, value);
        words.extend(std::iter::repeat_n(0x4e71, 64));
    }
    let mut words = vec![0x46fc, 0x2700];
    grant(&mut words);
    words.extend([0x33fc, 0x8104, 0x00c0, 0x0004]);
    for value in [0x9f, 0xbf, 0xdf, 0xff] {
        write(&mut words, 0xc00011, value);
    }
    for slot in [0, 4, 8, 12] {
        for (group, value) in [
            (0x30, 1),
            (0x40, 0),
            (0x50, 31),
            (0x60, 0),
            (0x70, 0),
            (0x80, 15),
            (0x90, 0),
        ] {
            timed_write(&mut words, group + slot + 2, value);
        }
    }
    for (register, value) in [
        (0xb2, 7),
        (0xb6, 0xc0),
        (0xa6, 0x22),
        (0xa2, 0x69),
        // A9, AA and A8 feed logical operators 1, 2 and 3 respectively.
        (0xad, 0x19),
        (0xa9, 0x45),
        (0xae, 0x2b),
        (0xaa, 0x78),
        (0xac, 0x32),
        (0xa8, 0xab),
        // Zero TL and stopped Timer A isolate frequency selection from CSM keying.
        (0x27, mode),
        (0x28, key_mask | 2),
    ] {
        timed_write(&mut words, register, value);
    }
    words.extend([0x60fe, 0x4e71]);
    words
}

pub fn channel_three_modes_share_special_frequency_ownership() {
    for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
        for concurrent in [false, true] {
            for key_mask in [0x10, 0x20, 0x40, 0x80] {
                let mut captures = Vec::new();
                for mode in [0, 0x40, 0x80, 0xc0] {
                    let words = channel_three_program(mode, key_mask);
                    let mut source = machine(&words, region, concurrent);
                    let cut = 3 * region.frame_ticks() + 42;
                    while source.timing_snapshot().now().get() < cut {
                        source.step_instruction().unwrap();
                    }
                    source.drain_audio_samples();
                    let saved = source.encode_state().unwrap();
                    let mut restored = machine(&words, region, concurrent);
                    restored.decode_state(&saved).unwrap();
                    assert_eq!(restored.encode_state().unwrap(), saved);
                    // Canonical states restart host resampling at the restore boundary.
                    source.set_sample_rate(source.sample_rate());
                    let end = cut + 3 * region.frame_ticks();
                    while source.timing_snapshot().now().get() < end {
                        assert_eq!(
                            source.step_instruction().unwrap(),
                            restored.step_instruction().unwrap()
                        );
                    }
                    assert_eq!(
                        source.encode_state().unwrap(),
                        restored.encode_state().unwrap()
                    );
                    let pcm = source.drain_audio_samples();
                    let restored_pcm = restored.drain_audio_samples();
                    assert!(
                        pcm == restored_pcm,
                        "restore PCM: {region:?}, concurrent={concurrent}, key={key_mask:#x}, mode={mode:#x}, lengths={}/{}, first={:?}",
                        pcm.len(),
                        restored_pcm.len(),
                        pcm.iter().zip(&restored_pcm).position(|(a, b)| a != b)
                    );
                    assert!(pcm.len() > 4_000);
                    assert!(pcm.iter().all(|sample| sample.is_finite()));
                    assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
                    assert!(
                        pcm.as_chunks::<2>()
                            .0
                            .iter()
                            .all(|frame| frame[0] == frame[1])
                    );
                    assert!(source.video_error().is_none());
                    assert!(source.sound_fault().is_none());
                    assert_eq!(source.sound_cpu().cycles(), 0);
                    assert!(source.sound_bus_requested() && source.sound_bus_granted());
                    assert!(!source.sound_reset_asserted());
                    assert_eq!(source.opn2_snapshot().status & 3, 0);
                    captures.push(pcm);
                }
                for index in [2, 3] {
                    assert!(
                        captures[1] == captures[index],
                        "special mode differs: {region:?}, concurrent={concurrent}, key={key_mask:#x}, mode={:#x}",
                        [0, 0x40, 0x80, 0xc0][index]
                    );
                }
                if key_mask == 0x80 {
                    assert!(
                        captures[0] == captures[1],
                        "operator 4 lost normal frequency"
                    );
                } else {
                    assert!(
                        captures[0] != captures[1],
                        "special operator kept normal frequency"
                    );
                }
            }
        }
    }
}
