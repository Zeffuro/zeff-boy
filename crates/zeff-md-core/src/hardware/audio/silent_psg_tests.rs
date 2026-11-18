use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

mod original;

fn fm_write(audio: &mut Audio, address: u8, value: u8) {
    audio.write_opn2(audio.now, 0, address);
    audio.write_opn2(audio.now, 1, value);
}

fn active(region: Region, rate: u32) -> Audio {
    let mut audio = Audio::new(region);
    audio.set_sample_rate(rate);
    for slot in [0, 4, 8, 12] {
        for (group, value) in [(0x30, 1), (0x40, 0), (0x50, 31)] {
            fm_write(&mut audio, group + slot, value);
        }
    }
    for (address, value) in [
        (0xb0, 7),
        (0xb4, 0xc0),
        (0xa4, 0x22),
        (0xa0, 0x69),
        (0x28, 0xf0),
        (0x2a, 0xc1),
        (0x2b, 0x80),
    ] {
        fm_write(&mut audio, address, value);
    }
    audio.advance_original(19_001);
    audio
}

fn set_psg(audio: &mut Audio, s: PsgDebugSnapshot) {
    let mut w = StateWriter::new();
    for value in s.periods {
        w.write_u16(value);
    }
    w.write_bytes(&s.volumes);
    for value in s.counters {
        w.write_u16(value);
    }
    for value in s.outputs {
        w.write_bool(value);
    }
    w.write_u8(s.latched_register);
    w.write_u8(s.noise_control);
    w.write_u16(s.noise_counter);
    w.write_bool(s.noise_clock);
    w.write_u16(s.noise_lfsr);
    audio.psg = Psg::read_state(&mut StateReader::new(&w.into_bytes())).unwrap();
    audio.restart_filter();
}

fn pair(seed: &Audio) -> (Audio, Audio) {
    (seed.clone(), seed.clone())
}

fn compare(actual: &mut Audio, expected: &mut Audio, at: u64, signature: &mut u64) {
    actual.advance_to(at);
    expected.advance_original(at);
    assert_eq!(actual, expected, "at={at}");
    *signature = signature.rotate_left(7).wrapping_add(actual.now)
        ^ actual.sample_phase
        ^ actual.chip_phase
        ^ (actual.len as u64)
        ^ u64::from(actual.psg.debug_snapshot().noise_lfsr);
}

pub(super) fn run_small() -> u64 {
    let mut signature = 0x24_02_16_65;
    let mut seed = active(Region::NtscUs, 48_000);
    let (mut actual, mut expected) = pair(&seed);
    for delta in [0, 1, 41, 42, 43, 239, 240, 241, 1008, 5003] {
        compare(&mut actual, &mut expected, seed.now + delta, &mut signature);
    }
    for audio in [&mut actual, &mut expected] {
        audio.write_psg(audio.now, 0x82);
        audio.write_psg(audio.now, 0);
        audio.write_psg(audio.now, 0x9e);
    }
    assert!(!actual.psg.silent_with_mutes(actual.mutes));
    let at = actual.now + 20_003;
    compare(&mut actual, &mut expected, at, &mut signature);
    for audio in [&mut actual, &mut expected] {
        audio.set_channel_mutes([true; PSG_CHANNELS]);
        audio.write_psg(audio.now, 0x90);
    }
    let at = actual.now + 5003;
    compare(&mut actual, &mut expected, at, &mut signature);
    for audio in [&mut actual, &mut expected] {
        audio.set_channel_mutes([false; PSG_CHANNELS]);
    }
    assert!(!actual.psg.silent_with_mutes(actual.mutes));
    let at = actual.now + 5003;
    compare(&mut actual, &mut expected, at, &mut signature);
    seed = actual;
    let mut pcm = Vec::new();
    seed.drain_into(&mut pcm);
    assert!(pcm.iter().any(|&v| v != 0.0));
    signature
}

fn all_silence_combinations(signature: &mut u64) {
    let seed = active(Region::NtscUs, 48_000);
    for mut choice in 0..17_u32.pow(4) {
        let mut actual = seed.clone();
        let mut s = actual.psg.debug_snapshot();
        s.periods = [2, 17, 1023];
        s.counters = [1023, 31, 7];
        for channel in 0..PSG_CHANNELS {
            let value = (choice % 17) as u8;
            choice /= 17;
            actual.mutes[channel] = value != 0;
            s.volumes[channel] = if value == 0 { 15 } else { value - 1 };
        }
        set_psg(&mut actual, s);
        assert!(actual.psg.silent_with_mutes(actual.mutes));
        let mut expected = actual.clone();
        compare(&mut actual, &mut expected, seed.now + 503, signature);
    }
}

fn one_audible_channel(signature: &mut u64) {
    let seed = active(Region::NtscUs, 48_000);
    for channel in 0..PSG_CHANNELS {
        for volume in 0..15 {
            let mut actual = seed.clone();
            let mut s = actual.psg.debug_snapshot();
            s.periods = [2, 3, 5];
            s.counters = [1, 2, 3];
            s.noise_control = 7;
            s.noise_lfsr = 0xa55a;
            s.volumes = [15; PSG_CHANNELS];
            s.volumes[channel] = volume;
            actual.mutes = [false; PSG_CHANNELS];
            set_psg(&mut actual, s);
            assert!(!actual.psg.silent_with_mutes(actual.mutes));
            let mut expected = actual.clone();
            compare(&mut actual, &mut expected, seed.now + 20_003, signature);
        }
    }
}

fn clock_domains(signature: &mut u64) {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for rate in [1, 44_100, 48_000, 192_000] {
            let seed = active(region, rate);
            for phase in 0..PSG_CLOCK_TICKS {
                for mode in 0..8 {
                    for high in [false, true] {
                        let mut actual = seed.clone();
                        let mut s = actual.psg.debug_snapshot();
                        s.periods = [0, 1, 2];
                        s.counters = [1023, 17, 3];
                        s.outputs = [high, !high, high];
                        s.noise_control = mode;
                        s.noise_counter = 64;
                        s.noise_clock = high;
                        s.noise_lfsr = if high { 0x8000 } else { 0xffff };
                        actual.mutes = [true, false, true, false];
                        s.volumes = [0, 15, 14, 15];
                        set_psg(&mut actual, s);
                        actual.chip_phase = phase;
                        actual.opn2_phase = phase % OPN2_CLOCK_TICKS;
                        actual.sample_phase = phase * u64::from(rate) % actual.master_hz;
                        let mut expected = actual.clone();
                        for delta in [0, 1, 239, 503, 2003] {
                            compare(&mut actual, &mut expected, seed.now + delta, signature);
                        }
                    }
                }
            }
        }
    }
}

fn deadline_ties(signature: &mut u64) {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for rate in [44_100, 48_000, 192_000] {
            let seed = active(region, rate);
            let fm = seed.opn2.clocks_to_sample() * OPN2_CLOCK_TICKS - seed.opn2_phase;
            assert!(fm < PSG_CLOCK_TICKS);
            for target in [fm, fm + 1] {
                for sample_tie in [false, true] {
                    let mut actual = seed.clone();
                    actual.chip_phase = PSG_CLOCK_TICKS - target;
                    if sample_tie {
                        actual.sample_phase = actual.master_hz - target * u64::from(rate);
                    }
                    let mut expected = actual.clone();
                    for delta in [target - 1, target, target + 1, target + 168, 5003] {
                        compare(&mut actual, &mut expected, seed.now + delta, signature);
                    }
                }
            }
        }
    }
}

fn codec_pair(actual: &mut Audio, expected: &mut Audio, region: Region, frontend: bool) {
    let mut a = StateWriter::new();
    let mut e = StateWriter::new();
    actual.write_state(&mut a).unwrap();
    expected.write_state(&mut e).unwrap();
    let a = a.into_bytes();
    let e = e.into_bytes();
    assert_eq!(a, e);
    let mut af = StateWriter::new();
    let mut ef = StateWriter::new();
    actual.write_frontend_state(&mut af).unwrap();
    expected.write_frontend_state(&mut ef).unwrap();
    let af = af.into_bytes();
    let ef = ef.into_bytes();
    assert_eq!(af, ef);
    for (audio, raw, host) in [(actual, a, af), (expected, e, ef)] {
        audio
            .read_state(&mut StateReader::new(&raw), region)
            .unwrap();
        if frontend {
            audio
                .read_frontend_state(&mut StateReader::new(&host))
                .unwrap();
        }
    }
}

fn lifecycle(signature: &mut u64) {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for rate in [44_100, 48_000, 192_000] {
            for frontend in [false, true] {
                let seed = active(region, rate);
                let (mut actual, mut expected) = pair(&seed);
                let retained = (SAMPLE_CAPACITY + output::HISTORY) as u64;
                for count in [retained - 1, retained, retained + 1, retained * 2] {
                    let delta = (u128::from(count) * u128::from(actual.master_hz)
                        - u128::from(actual.sample_phase))
                    .div_ceil(u128::from(rate)) as u64;
                    let at = actual.now + delta;
                    compare(&mut actual, &mut expected, at, signature);
                    codec_pair(&mut actual, &mut expected, region, frontend);
                    assert_eq!(actual, expected);
                    let mut pcm = Vec::new();
                    let mut control = Vec::new();
                    actual.drain_into(&mut pcm);
                    expected.drain_into(&mut control);
                    assert_eq!(pcm, control);
                    for audio in [&mut actual, &mut expected] {
                        audio.set_sample_generation_enabled(false);
                    }
                    let at = actual.now + 5003;
                    compare(&mut actual, &mut expected, at, signature);
                    for audio in [&mut actual, &mut expected] {
                        audio.set_sample_generation_enabled(true);
                        audio.write_psg(audio.now, 0x9e);
                        audio.set_channel_mutes([true; PSG_CHANNELS]);
                        fm_write(audio, 0xb4, 0x80);
                        audio.reset_opn2(audio.now);
                    }
                    let at = actual.now + 3001;
                    compare(&mut actual, &mut expected, at, signature);
                }
                for audio in [&mut actual, &mut expected] {
                    audio.reset();
                }
                compare(&mut actual, &mut expected, 5003, signature);
            }
        }
    }
}

pub(super) fn run_all() -> u64 {
    let mut signature = run_small();
    all_silence_combinations(&mut signature);
    one_audible_channel(&mut signature);
    clock_domains(&mut signature);
    deadline_ties(&mut signature);
    lifecycle(&mut signature);
    signature
}

#[test]
fn silent_psg_small_original_loop_differential() {
    run_small();
}

#[test]
fn silent_psg_shared_original_loop_regressions() {
    run_all();
}
