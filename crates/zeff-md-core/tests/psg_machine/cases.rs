use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_md_core::constants::NTSC_MASTER_HZ;
use zeff_md_core::{Emulator, LoadConfig};

pub fn image(noise: bool) -> Vec<u8> {
    let mut words = Vec::new();
    let values: &[u16] = if noise {
        &[0xe4, 0xf0]
    } else {
        &[0x85, 0x06, 0x90]
    };
    for &value in values {
        words.extend([0x13fc, value, 0x00c0, 0x0011]);
    }
    words.extend([0x60fe, 0x4e71]);
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in words.into_iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    image
}

fn run(machine: &mut Emulator, end: u64) {
    while machine.timing_snapshot().now().get() < end {
        machine.step_instruction().unwrap();
    }
}

pub fn cpu_writes_make_clocked_stereo() {
    let mut expected = None;
    for concurrent_sound in [false, true] {
        let mut machine = Emulator::new(
            image(false),
            LoadConfig {
                concurrent_sound,
                ..Default::default()
            },
        )
        .unwrap();
        run(&mut machine, NTSC_MASTER_HZ / 100);
        assert_eq!(machine.psg_snapshot().periods[0], 101);
        assert_eq!(machine.psg_snapshot().volumes, [0, 15, 15, 15]);
        let samples = machine.drain_audio_samples();
        let count = machine.timing_snapshot().now().get() * 48_000 / NTSC_MASTER_HZ;
        assert_eq!(samples.len() as u64, count * 2);
        assert!(
            samples
                .as_chunks::<2>()
                .0
                .iter()
                .all(|pair| pair[0] == pair[1])
        );
        assert!(samples.iter().any(|&sample| sample > 0.0));
        assert!(samples.iter().any(|&sample| sample < 0.0));
        let crossings = samples
            .get(96..)
            .unwrap()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| pair[0] > 0.0)
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|pair| pair[0] != pair[1])
            .count();
        assert!((19..=21).contains(&crossings), "{crossings}");
        if let Some(expected) = &expected {
            assert_eq!(&samples, expected);
        }
        expected = Some(samples);
    }
}

pub fn noise_and_sound_reset_ownership() {
    for concurrent_sound in [false, true] {
        let mut machine = Emulator::new(
            image(true),
            LoadConfig {
                concurrent_sound,
                ..Default::default()
            },
        )
        .unwrap();
        run(&mut machine, NTSC_MASTER_HZ / 100);
        assert_eq!(machine.psg_snapshot().noise_control, 4);
        assert_ne!(machine.psg_snapshot().noise_lfsr, 0x8000);
        let before = machine.psg_snapshot();
        machine.reset_sound_cpu();
        assert_eq!(machine.psg_snapshot(), before);
        let output = machine.drain_audio_samples();
        assert!(output.iter().any(|&sample| sample > 0.0));
        assert!(output.iter().any(|&sample| sample < 0.0));
        assert!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .all(|pair| pair[0] == pair[1])
        );
    }
}

pub fn drain_rate_clone_and_reset() {
    let mut machine = Emulator::new(image(false), LoadConfig::default()).unwrap();
    machine.set_sample_rate(44_100);
    run(&mut machine, NTSC_MASTER_HZ / 100);
    let mut copy = machine.clone();
    let mut output = Vec::with_capacity(2000);
    let pointer = output.as_ptr();
    machine.drain_audio_samples_into(&mut output);
    assert_eq!(output, copy.drain_audio_samples());
    assert_eq!(output.as_ptr(), pointer);
    let end = machine.timing_snapshot().now().get() + NTSC_MASTER_HZ / 100;
    run(&mut machine, end);
    run(&mut copy, end);
    assert_eq!(machine, copy);
    machine.reset_cpu().unwrap();
    assert_eq!(machine.psg_snapshot().volumes[0], 0);
    machine.set_sample_rate(0);
    assert_eq!(machine.sample_rate(), 48_000);
    assert!(machine.drain_audio_samples().is_empty());
    machine.set_sample_rate(u32::MAX);
    assert_eq!(machine.sample_rate(), 192_000);
    Reset::reset(&mut machine);
    assert_eq!(machine.psg_snapshot().volumes, [15; 4]);
    assert_eq!(machine.sample_rate(), 192_000);
    assert!(machine.drain_audio_samples().is_empty());
}

pub fn sound_image() -> Vec<u8> {
    let mut words = Vec::new();
    let mut write = |value: u8, address: u32| {
        words.extend([
            0x13fc,
            u16::from(value),
            (address >> 16) as u16,
            address as u16,
        ]);
    };
    write(1, 0xa11200);
    write(1, 0xa11100);
    for (address, value) in [
        0x3e, 0x85, 0x32, 0x11, 0x7f, 0x3e, 0x06, 0x32, 0x11, 0x7f, 0x3e, 0x90, 0x32, 0x11, 0x7f,
        0x76,
    ]
    .into_iter()
    .enumerate()
    {
        write(value, 0xa00000 + address as u32);
    }
    write(0, 0xa11200);
    write(0, 0xa11100);
    write(1, 0xa11200);
    words.extend([0x4e72, 0x2700, 0x4e71, 0x4e71]);
    let mut image = vec![0; 0x400];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in words.into_iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    image
}

pub fn actual_sound_cpu_pcm() {
    for concurrent_sound in [false, true] {
        let mut machine = Emulator::new(
            sound_image(),
            LoadConfig {
                concurrent_sound,
                ..Default::default()
            },
        )
        .unwrap();
        while machine.step_instruction().unwrap().cpu != zeff_m68k::timing::TimedOutcome::Stopped {}
        while machine.timing_snapshot().now().get() < NTSC_MASTER_HZ / 100 {
            if concurrent_sound {
                machine.step_instruction().unwrap();
            } else {
                machine.step_sound_instruction().unwrap();
            }
        }
        assert_eq!(machine.psg_snapshot().periods[0], 101);
        assert_eq!(machine.psg_snapshot().volumes, [0, 15, 15, 15]);
        assert!(machine.sound_cpu().cycles() > 100);
        assert!(machine.sound_fault().is_none());
        let output = machine.drain_audio_samples();
        assert!(output.iter().any(|&sample| sample > 0.0));
        assert!(output.iter().any(|&sample| sample < 0.0));
        assert!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .all(|pair| pair[0] == pair[1])
        );
        let count = machine.timing_snapshot().now().get() * 48_000 / NTSC_MASTER_HZ;
        assert_eq!(output.len() as u64, count * 2);
    }
}

pub fn sample_controls_keep_hardware_running() {
    let mut ordinary = Emulator::new(image(true), LoadConfig::default()).unwrap();
    let mut disabled = ordinary.clone();
    let mut muted = ordinary.clone();
    disabled.set_apu_sample_generation_enabled(false);
    muted.set_apu_channel_mutes([true; 4]);
    run(&mut ordinary, NTSC_MASTER_HZ / 100);
    run(&mut disabled, NTSC_MASTER_HZ / 100);
    run(&mut muted, NTSC_MASTER_HZ / 100);
    assert_eq!(ordinary.psg_snapshot(), disabled.psg_snapshot());
    assert_eq!(ordinary.psg_snapshot(), muted.psg_snapshot());
    assert!(disabled.drain_audio_samples().is_empty());
    let mut output = vec![0.25, -0.25];
    muted.drain_audio_samples_into(&mut output);
    assert_eq!(&output[..2], &[0.25, -0.25]);
    assert!(output[2..].iter().all(|&sample| sample == 0.0));
    muted.drain_audio_samples_into(&mut output);
    assert_eq!(&output[..2], &[0.25, -0.25]);
    disabled.set_apu_sample_generation_enabled(true);
    muted.set_apu_channel_mutes([false; 4]);
    ordinary.drain_audio_samples();
    run(&mut ordinary, NTSC_MASTER_HZ / 50);
    run(&mut disabled, NTSC_MASTER_HZ / 50);
    run(&mut muted, NTSC_MASTER_HZ / 50);
    let output = ordinary.drain_audio_samples();
    let disabled = disabled.drain_audio_samples();
    let muted = muted.drain_audio_samples();
    assert_eq!(output[96..], disabled[96..]);
    assert_eq!(output[96..], muted[96..]);
}
