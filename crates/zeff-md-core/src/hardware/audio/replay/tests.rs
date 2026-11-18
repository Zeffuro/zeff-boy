use super::*;
use crate::hardware::audio::{Opn2, output};
use zeff_emu_common::audio_trace::{MdTraceOrigin, MdTraceSource, MdTraceWriter};

const ORIGIN: MdTraceOrigin = MdTraceOrigin {
    writer: MdTraceWriter::M68000,
    boundary_pc: 0x200,
    source: MdTraceSource::EffectiveRom { offset: 0x200 },
};

fn psg(value: u8) -> MdTraceWrite {
    MdTraceWrite::Psg {
        origin: ORIGIN,
        value,
    }
}

fn opn2(port: u8, value: u8) -> MdTraceWrite {
    MdTraceWrite::Opn2Port {
        origin: ORIGIN,
        port,
        value,
    }
}

fn reset() -> MdTraceWrite {
    MdTraceWrite::Opn2Reset { origin: ORIGIN }
}

fn direct_write(audio: &mut Audio, at: u64, write: MdTraceWrite, pcm: &mut Vec<f32>) {
    match write {
        MdTraceWrite::Psg { value, .. } => audio.write_psg(at, value),
        MdTraceWrite::Opn2Port { port, value, .. } => audio.write_opn2(at, port, value),
        MdTraceWrite::Opn2Reset { .. } => audio.reset_opn2(at),
    }
    audio.drain_into(pcm);
}

fn register(replay: &mut MdAudioReplay, at: u64, address: u8, value: u8) {
    replay.write(at, opn2(0, address), &mut Vec::new()).unwrap();
    replay.write(at, opn2(1, value), &mut Vec::new()).unwrap();
}

#[test]
fn constructor_accepts_exact_regional_descriptors_and_rate_endpoints() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let chip = trace_chip(region);
        for rate in [1, 44_100, 48_000, 63_072, 96_000, 192_000] {
            let replay = MdAudioReplay::new(&chip, rate).unwrap();
            assert_eq!(replay.now(), 0);
            assert_eq!(replay.sample_rate(), rate);
            let mut expected = Audio::new(region);
            expected.set_sample_rate(rate);
            assert_eq!(replay.audio, expected);
        }
        for rate in [0, 192_001, u32::MAX] {
            assert!(MdAudioReplay::new(&chip, rate).is_err());
        }
        let mut alterations = Vec::new();
        for master_hz in [0, chip.master_hz + 1, u64::MAX] {
            alterations.push(MdTraceChip { master_hz, ..chip });
        }
        for psg_divisor in [0, 15, 239, 241, u32::MAX] {
            alterations.push(MdTraceChip {
                psg_divisor,
                ..chip
            });
        }
        for opn2_divisor in [0, 6, 8, u32::MAX] {
            alterations.push(MdTraceChip {
                opn2_divisor,
                ..chip
            });
        }
        alterations.push(MdTraceChip {
            region: if chip.region == MdTraceRegion::Pal {
                MdTraceRegion::NtscUs
            } else {
                MdTraceRegion::Pal
            },
            ..chip
        });
        for invalid in alterations {
            assert!(MdAudioReplay::new(&invalid, 48_000).is_err());
        }
    }
}

#[test]
fn failed_steps_and_writes_preserve_all_state_and_existing_output() {
    let chip = trace_chip(Region::NtscUs);
    let mut replay = MdAudioReplay::new(&chip, 48_000).unwrap();
    let mut pcm = vec![0.25, -0.5];
    replay.write(0, psg(0x90), &mut pcm).unwrap();
    replay.advance_to(1_237, &mut pcm).unwrap();
    let before = replay.audio.clone();
    let output_before = pcm.clone();
    let too_far = (chip.master_hz * 1026).div_ceil(48_000);
    for at in [0, replay.now() - 1, too_far, u64::MAX] {
        assert!(replay.advance_to(at, &mut pcm).is_err());
        assert_eq!(replay.audio, before);
        assert_eq!(pcm, output_before);
        for write in [psg(0x9f), opn2(0, 0x27), reset()] {
            assert!(replay.write(at, write, &mut pcm).is_err());
            assert_eq!(replay.audio, before);
            assert_eq!(pcm, output_before);
        }
    }
    for port in [4, 5, 254, 255] {
        for at in [0, replay.now(), replay.now() + 100_000, u64::MAX] {
            assert!(replay.write(at, opn2(port, 0xff), &mut pcm).is_err());
            assert_eq!(replay.audio, before);
            assert_eq!(pcm, output_before);
        }
    }
    replay.advance_to(replay.now(), &mut pcm).unwrap();
    assert_eq!(replay.audio, before);
    assert_eq!(pcm, output_before);
}

#[test]
fn frame_limit_uses_retained_phase_and_drains_every_successful_call() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for rate in [44_100, 48_000, 63_072, 96_000, 192_000] {
            let chip = trace_chip(region);
            let mut replay = MdAudioReplay::new(&chip, rate).unwrap();
            let mut pcm = vec![0.125, -0.25];
            replay.advance_to(56, &mut pcm).unwrap();
            assert!(replay.audio.sample_phase > 0);
            let first =
                (chip.master_hz * MD_AUDIO_REPLAY_MAX_FRAMES as u64).div_ceil(u64::from(rate));
            replay.advance_to(first, &mut pcm).unwrap();
            assert_eq!(pcm.len(), 2 + MD_AUDIO_REPLAY_MAX_FRAMES * 2);
            assert_eq!(&pcm[..2], &[0.125, -0.25]);
            assert_eq!(replay.audio.len, 0);
            let before = replay.audio.clone();
            let saved_pcm = pcm.clone();
            let forbidden = (chip.master_hz * (MD_AUDIO_REPLAY_MAX_FRAMES as u64 * 2 + 1))
                .div_ceil(u64::from(rate));
            assert!(replay.advance_to(forbidden, &mut pcm).is_err());
            assert_eq!(replay.audio, before);
            assert_eq!(pcm, saved_pcm);
            let accepted =
                (chip.master_hz * MD_AUDIO_REPLAY_MAX_FRAMES as u64 * 2).div_ceil(u64::from(rate));
            replay.advance_to(accepted, &mut pcm).unwrap();
            assert_eq!(pcm.len(), 2 + MD_AUDIO_REPLAY_MAX_FRAMES * 4);
            assert_eq!(replay.audio.len, 0);
        }
    }
}

#[test]
fn repeated_bounded_advances_keep_every_frame_beyond_ring_capacity() {
    let region = Region::Pal;
    let chip = trace_chip(region);
    let mut replay = MdAudioReplay::new(&chip, 48_000).unwrap();
    let mut direct = Audio::new(region);
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for value in [0x85, 0x06, 0x90] {
        replay.write(0, psg(value), &mut actual).unwrap();
        direct.write_psg(0, value);
    }
    for part in 1..=12 {
        let at = (chip.master_hz * 777 * part).div_ceil(48_000);
        replay.advance_to(at, &mut actual).unwrap();
        let middle = direct.now + (at - direct.now) / 2;
        direct.advance_to(middle);
        direct.drain_into(&mut expected);
        direct.advance_to(at);
        direct.drain_into(&mut expected);
        assert_eq!(actual, expected);
        assert_eq!(replay.audio.len, 0);
        assert_eq!(replay.audio.psg, direct.psg);
        assert_eq!(replay.audio.output, direct.output);
    }
    assert_eq!(actual.len(), 12 * 777 * 2);
    assert!(actual.iter().any(|value| *value != 0.0));
}

#[test]
fn ordered_writes_match_native_audio_across_regions_rates_and_clock_edges() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for rate in [44_100, 48_000, 63_072, 96_000] {
            let chip = trace_chip(region);
            let mut replay = MdAudioReplay::new(&chip, rate).unwrap();
            let mut direct = Audio::new(region);
            direct.set_sample_rate(rate);
            let mut actual = vec![0.25];
            let mut expected = actual.clone();
            let sample_edge = chip.master_hz.div_ceil(u64::from(rate));
            let mut events = [
                (0, psg(0x85)),
                (0, psg(0x06)),
                (0, psg(0x90)),
                (0, opn2(2, 0xb6)),
                (0, opn2(1, 0x40)),
                (0, opn2(0, 0x2b)),
                (0, opn2(3, 0x80)),
                (7, opn2(0, 0x2a)),
                (239, psg(0xa7)),
                (240, psg(0x03)),
                (1007, opn2(3, 0xff)),
                (1008, opn2(1, 0x00)),
                (sample_edge - 1, psg(0xb0)),
                (sample_edge, opn2(1, 0xff)),
                (sample_edge, opn2(0, 0x2a)),
                (sample_edge, opn2(3, 0x81)),
                (sample_edge + 1, psg(0x92)),
                (123_457, reset()),
                (123_457, opn2(3, 0x12)),
                (123_458, psg(0x94)),
                (500_003, reset()),
            ];
            events.sort_by_key(|(at, _)| *at);
            for (at, write) in events {
                replay.write(at, write, &mut actual).unwrap();
                direct_write(&mut direct, at, write, &mut expected);
                assert_eq!(actual, expected);
                assert_eq!(replay.audio, direct);
                assert_eq!(replay.audio.len, 0);
            }
            replay.advance_to(700_013, &mut actual).unwrap();
            direct.advance_to(700_013);
            direct.drain_into(&mut expected);
            assert_eq!(actual, expected);
            assert!(actual[1..].iter().any(|value| *value != 0.0));
            assert_eq!(replay.audio, direct);
        }
    }
}

#[test]
fn same_tick_cross_bank_latch_order_and_sample_endpoint_are_preserved() {
    let chip = trace_chip(Region::NtscUs);
    let mut replay = MdAudioReplay::new(&chip, 48_000).unwrap();
    let mut pcm = Vec::new();
    for (port, value) in [(2, 0xb6), (1, 0x40), (0, 0x2b), (3, 0x80)] {
        replay.write(0, opn2(port, value), &mut pcm).unwrap();
    }
    assert!(replay.audio.opn2.debug_snapshot().dac_enabled);
    let at = chip.master_hz.div_ceil(48_000);
    replay.write(at, opn2(0, 0x2a), &mut pcm).unwrap();
    replay.write(at, opn2(3, 255), &mut pcm).unwrap();
    assert_eq!(pcm, [0.0; 2]);
    assert_eq!(replay.audio.opn2.debug_snapshot().dac, 255);
    replay
        .advance_to(at * (output::HISTORY as u64 + 4), &mut pcm)
        .unwrap();
    assert!(pcm.as_chunks::<2>().0.iter().all(|pair| pair[0] == 0.0));
    assert!(
        pcm[(output::HISTORY + 1) * 2..]
            .as_chunks::<2>()
            .0
            .iter()
            .all(|pair| pair[1] == 8128.0 / 32768.0)
    );
}

#[test]
fn opn2_reset_retains_psg_sample_phase_time_and_filter_history() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut replay = MdAudioReplay::new(&trace_chip(region), 44_100).unwrap();
        let mut pcm = Vec::new();
        for value in [0x85, 0x06, 0x90] {
            replay.write(0, psg(value), &mut pcm).unwrap();
        }
        register(&mut replay, 0, 0x2a, 255);
        register(&mut replay, 0, 0x2b, 0x80);
        replay.advance_to(123_457, &mut pcm).unwrap();
        let before = replay.audio.clone();
        let mut expected = before.clone();
        expected.reset_opn2(expected.now);
        replay.write(123_457, reset(), &mut pcm).unwrap();
        assert_eq!(replay.audio, expected);
        assert_eq!(replay.audio.opn2, Opn2::default());
        assert_eq!(replay.audio.opn2_phase, 0);
        assert_eq!(replay.audio.psg, before.psg);
        assert_eq!(replay.audio.chip_phase, before.chip_phase);
        assert_eq!(replay.audio.sample_phase, before.sample_phase);
        assert_eq!(replay.now(), before.now);
        assert_ne!(replay.audio.output, Audio::new(region).output);
        let mut expected_pcm = pcm.clone();
        replay.advance_to(500_003, &mut pcm).unwrap();
        expected.advance_to(500_003);
        expected.drain_into(&mut expected_pcm);
        assert_eq!(pcm, expected_pcm);
        assert_eq!(replay.audio, expected);
    }
}
