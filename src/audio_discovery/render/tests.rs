use super::*;
use crate::audio_discovery::mp2k::{Program, TimedEvent};

fn note(tick: u32, key: u8, gate: u16) -> TimedEvent {
    TimedEvent {
        tick,
        event: Event::Note {
            key,
            velocity: 100,
            gate,
        },
    }
}

#[test]
fn playback_gain_mapping_is_exhaustive_monotone_and_serialized() -> Result<()> {
    assert_eq!(RenderOptions::default().playback_gain, PlaybackGain::Raw);
    assert_eq!(
        PlaybackGain::ALL,
        [PlaybackGain::Raw, PlaybackGain::Mp2kAmplitude]
    );
    assert_eq!(PlaybackGain::parse("raw")?, PlaybackGain::Raw);
    assert_eq!(PlaybackGain::parse("mp2k")?, PlaybackGain::Mp2kAmplitude);
    assert!(PlaybackGain::parse("linear").is_err());
    assert_eq!(PlaybackGain::Raw.id(), "raw");
    assert_eq!(PlaybackGain::Mp2kAmplitude.id(), "mp2k");
    assert_eq!(PlaybackGain::Raw.label(), "Raw controls");
    assert_eq!(PlaybackGain::Mp2kAmplitude.label(), "MP2K amplitude");

    let mut previous = 0;
    for input in 0..=127 {
        assert_eq!(PlaybackGain::Raw.map(input), input);
        let mapped = PlaybackGain::Mp2kAmplitude.map(input);
        let expected = (f64::from(127 * u16::from(input))).sqrt().round() as u8;
        assert_eq!(mapped, expected, "input {input}");
        assert!(mapped >= previous, "mapping decreased at input {input}");
        previous = mapped;
    }
    assert_eq!(PlaybackGain::Mp2kAmplitude.map(0), 0);
    assert_eq!(PlaybackGain::Mp2kAmplitude.map(127), 127);
    assert_eq!(
        serde_json::to_value(PlaybackGain::Mp2kAmplitude)?,
        serde_json::json!("mp2k")
    );
    Ok(())
}

#[test]
fn shorter_track_loops_repeat_to_the_common_song_end() -> Result<()> {
    let short = Program {
        events: vec![note(0, 60, 1), note(4, 61, 1)],
        ticks: 8,
        loop_start: Some(0),
        loop_event_start: Some(0),
    };
    let long = Program {
        events: vec![note(0, 70, 1)],
        ticks: 16,
        loop_start: Some(0),
        loop_event_start: Some(0),
    };
    let (timeline, end) = build_timeline(&[short, long], 2)?;
    assert_eq!(end, 32);
    let short_note_ticks = timeline
        .iter()
        .filter_map(|event| match event.event {
            ScheduledEvent::Sequence(Event::Note { .. }) if event.track == 0 => Some(event.tick),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(short_note_ticks, [0, 4, 8, 12, 16, 20, 24, 28]);
    Ok(())
}

#[test]
fn gate_offs_sort_before_new_notes_on_the_same_tick() -> Result<()> {
    let program = Program {
        events: vec![note(0, 60, 4), note(4, 60, 4)],
        ticks: 8,
        loop_start: None,
        loop_event_start: None,
    };
    let (timeline, _) = build_timeline(&[program], 1)?;
    let at_four = timeline
        .iter()
        .filter(|event| event.tick == 4)
        .collect::<Vec<_>>();
    assert!(matches!(
        at_four[0].event,
        ScheduledEvent::GateOff { key: 60 }
    ));
    assert!(matches!(
        at_four[1].event,
        ScheduledEvent::Sequence(Event::Note { key: 60, .. })
    ));
    Ok(())
}

#[test]
fn loop_event_index_separates_same_tick_intro_from_loop_body() -> Result<()> {
    let program = Program {
        events: vec![
            TimedEvent {
                tick: 4,
                event: Event::Control {
                    opcode: 0xBE,
                    value: 90,
                },
            },
            note(4, 60, 1),
            TimedEvent {
                tick: 8,
                event: Event::Control {
                    opcode: 0xBF,
                    value: 64,
                },
            },
        ],
        ticks: 8,
        loop_start: Some(4),
        loop_event_start: Some(1),
    };
    let (timeline, end) = build_timeline(&[program], 2)?;
    assert_eq!(end, 12);
    let sequence_ticks = timeline
        .iter()
        .filter_map(|event| match event.event {
            ScheduledEvent::Sequence(_) => Some(event.tick),
            ScheduledEvent::GateOff { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(sequence_ticks, [4, 4, 8, 8, 12]);
    Ok(())
}

#[test]
fn end_tick_events_do_not_let_gate_offs_extend_the_song() -> Result<()> {
    let program = Program {
        events: vec![
            note(7, 60, 96),
            TimedEvent {
                tick: 8,
                event: Event::Fine,
            },
        ],
        ticks: 8,
        loop_start: None,
        loop_event_start: None,
    };
    let (timeline, end_tick) = build_timeline(&[program], 1)?;
    assert_eq!(end_tick, 8);
    assert!(timeline.iter().any(|event| {
        event.tick == 8 && matches!(event.event, ScheduledEvent::Sequence(Event::Fine))
    }));
    assert!(timeline.iter().all(|event| event.tick <= end_tick));
    let (_, end_frame) = map_frames(timeline, end_tick, 48_000)?;
    let (_, expected_frame) = map_frames(Vec::new(), end_tick, 48_000)?;
    assert_eq!(end_frame, expected_frame);
    Ok(())
}

#[test]
fn default_tempo_matches_one_tick_per_gba_frame() -> Result<()> {
    let (_, frame) = map_frames(Vec::new(), 60, 48_000)?;
    let expected = (60.0_f64 * 48_000.0 * 280_896.0 / 16_777_216.0).round() as usize;
    assert_eq!(frame, (expected + 4) / 8 * 8);
    Ok(())
}

fn render_fixture(shift: u8) -> Result<RenderedAudio> {
    render_fixture_rate(shift, 8_000)
}

fn render_fixture_rate(shift: u8, rate: u32) -> Result<RenderedAudio> {
    render_fixture_controls(shift, rate, PlaybackGain::Raw, 127, 127)
}

fn render_fixture_controls(
    shift: u8,
    rate: u32,
    playback_gain: PlaybackGain,
    velocity: u8,
    volume: u8,
) -> Result<RenderedAudio> {
    render_fixture_options(
        shift,
        rate,
        velocity,
        volume,
        RenderOptions {
            max_seconds: 2,
            playback_gain,
            ..RenderOptions::default()
        },
    )
}

fn fixture_inputs(
    shift: u8,
    rate: u32,
    velocity: u8,
    volume: u8,
) -> Result<(SongCandidate, Vec<u8>, Vec<u8>)> {
    fixture_inputs_with_tracks(shift, rate, velocity, volume, 1)
}

fn fixture_inputs_with_tracks(
    shift: u8,
    rate: u32,
    velocity: u8,
    volume: u8,
    tracks: u8,
) -> Result<(SongCandidate, Vec<u8>, Vec<u8>)> {
    use crate::audio_discovery::{projection, sf2, test_support};
    const WAIT_48: u8 = 0xA0;
    let mut bytes = test_support::fixture();
    bytes[0x100] = tracks;
    bytes[0x208..0x20C].copy_from_slice(&[255, 255, 255, 128]);
    test_support::put_word(&mut bytes, 0x304, rate * 1024);
    test_support::put_word(&mut bytes, 0x308, 0);
    test_support::put_word(&mut bytes, 0x30C, 64);
    for index in 0..64 {
        bytes[0x310 + index] = if index < 32 { 100 } else { 156 };
    }
    let sequence = [
        0xBD, 0, 0xBB, 60, 0xBE, volume, 0xBC, shift, 0xFF, 60, velocity, WAIT_48, 0xB1,
    ];
    bytes[0x400..0x400 + sequence.len()].copy_from_slice(&sequence);
    if tracks == 2 {
        let tempo_track = [
            0xBD, 0, 0xBB, 120, 0xBE, volume, 0xFF, 67, velocity, WAIT_48, 0xB1,
        ];
        bytes[0x440..0x440 + tempo_track.len()].copy_from_slice(&tempo_track);
    }
    let report = crate::audio_discovery::scan(
        zeff_emu_common::system::System::Gba,
        &bytes,
        Default::default(),
        &AtomicBool::new(false),
    );
    let song = report.candidates[0].clone();
    let bank = sf2::encode(&projection::instrument_bank(
        &bytes,
        &song,
        "fixture".to_owned(),
        None,
    )?)?;
    Ok((song, bytes, bank))
}

fn two_track_fixture_inputs() -> Result<(SongCandidate, Vec<u8>, Vec<u8>)> {
    fixture_inputs_with_tracks(0, 8_000, 127, 127, 2)
}

fn render_fixture_options(
    shift: u8,
    rate: u32,
    velocity: u8,
    volume: u8,
    options: RenderOptions,
) -> Result<RenderedAudio> {
    let (song, bytes, bank) = fixture_inputs(shift, rate, velocity, volume)?;
    render(
        &song,
        &bytes,
        &bank,
        options,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )
}
fn steady_left_rms(rendered: &RenderedAudio) -> f64 {
    let points = rendered
        .pcm
        .as_chunks::<2>()
        .0
        .iter()
        .skip(10_000)
        .take(20_000)
        .map(|frame| f64::from(frame[0]));
    let (sum, count) = points.fold((0.0, 0usize), |(sum, count), point| {
        (point.mul_add(point, sum), count + 1)
    });
    (sum / count as f64).sqrt()
}

#[test]
fn mp2k_playback_gain_linearizes_synth_velocity_and_cc7_amplitude() -> Result<()> {
    for input in [32, 64, 127] {
        for (velocity, volume) in [(input, 127), (127, input)] {
            let raw = render_fixture_controls(0, 8_000, PlaybackGain::Raw, velocity, volume)?;
            let shaped =
                render_fixture_controls(0, 8_000, PlaybackGain::Mp2kAmplitude, velocity, volume)?;
            let raw_rms = steady_left_rms(&raw);
            let shaped_rms = steady_left_rms(&shaped);
            let mapped = f64::from(PlaybackGain::Mp2kAmplitude.map(input));
            let expected_ratio = (mapped / f64::from(input)).powi(2);
            let actual_ratio = shaped_rms / raw_rms;
            assert!(
                (actual_ratio / expected_ratio - 1.0).abs() < 0.01,
                "input {input}, velocity {velocity}, volume {volume}: expected RMS ratio {expected_ratio}, got {actual_ratio}"
            );
            if input == 127 {
                assert_eq!(shaped.pcm, raw.pcm);
            } else {
                assert!(shaped_rms > raw_rms);
            }
        }
    }
    Ok(())
}

#[test]
fn rendered_pcm_is_stereo_deterministic_and_tunes_without_losing_the_key_zone() -> Result<()> {
    let original = render_fixture(0)?;
    let repeated = render_fixture(0)?;
    let octave = render_fixture(12)?;
    assert_eq!(original.sample_rate, 48_000);
    assert!(!original.warnings.is_empty());
    assert_eq!(original.pcm, repeated.pcm);
    assert!(original.pcm.iter().any(|point| point.unsigned_abs() > 1000));
    let (left, right) =
        original
            .pcm
            .as_chunks::<2>()
            .0
            .iter()
            .fold((0u64, 0u64), |(left, right), frame| {
                (
                    left + u64::from(frame[0].unsigned_abs()),
                    right + u64::from(frame[1].unsigned_abs()),
                )
            });
    assert!(
        (left as f64 / right as f64 - 1.0).abs() < 0.02,
        "center pan should have balanced channel energy"
    );
    let positive_crossings = |pcm: &[i16]| {
        pcm.as_chunks::<2>()
            .0
            .iter()
            .skip(10_000)
            .take(20_000)
            .map(|frame| frame[0])
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|pair| pair[0] <= 0 && pair[1] > 0)
            .count()
    };
    let base = positive_crossings(&original.pcm);
    let shifted = positive_crossings(&octave.pcm);
    assert!(
        (50..=54).contains(&base),
        "expected a 125 Hz fundamental, got {base} crossings"
    );
    assert!(
        shifted.abs_diff(base * 2) <= 2,
        "octave shift: {base} -> {shifted}"
    );
    Ok(())
}

#[test]
fn projected_rate_changes_preserve_audible_pitch() -> Result<()> {
    for (rate, shift) in [(53_516, 0), (63_072, 0), (375, 24)] {
        let rendered = render_fixture_rate(shift, rate)?;
        let left = rendered
            .pcm
            .as_chunks::<2>()
            .0
            .iter()
            .skip(10_000)
            .take(20_000)
            .map(|frame| frame[0])
            .collect::<Vec<_>>();
        let crossings = left
            .windows(2)
            .filter(|pair| pair[0] <= 0 && pair[1] > 0)
            .count();
        let expected =
            f64::from(rate) / 64.0 * 2f64.powf(f64::from(shift) / 12.0) * 20_000.0 / 48_000.0;
        assert!(
            (crossings as f64 - expected).abs() <= 2.0,
            "rate {rate}, shift {shift}: {crossings} crossings, expected {expected}"
        );
    }
    Ok(())
}

#[test]
fn output_rates_preserve_sequence_duration_and_audible_pitch() -> Result<()> {
    for sample_rate in SAMPLE_RATES {
        let rendered = render_fixture_options(
            0,
            8_000,
            127,
            127,
            RenderOptions {
                sample_rate,
                ..RenderOptions::default()
            },
        )?;
        assert_eq!(rendered.sample_rate, sample_rate);
        let expected = (60 * u128::from(sample_rate) * 280_896 / 16_777_216) as usize;
        assert_eq!(rendered.pcm.len() / 2, (expected + 4) / 8 * 8);
        let start = sample_rate as usize / 5;
        let count = sample_rate as usize * 3 / 10;
        let points = rendered.pcm.as_chunks::<2>().0;
        let crossings = points[start..start + count]
            .windows(2)
            .filter(|pair| pair[0][0] <= 0 && pair[1][0] > 0)
            .count();
        assert!(
            (36..=39).contains(&crossings),
            "{sample_rate} Hz: {crossings} crossings"
        );
    }
    Ok(())
}

#[test]
fn tempo_changes_use_the_selected_output_clock() -> Result<()> {
    for sample_rate in SAMPLE_RATES {
        let timeline = vec![
            Scheduled {
                tick: 0,
                track: 0,
                order: 0,
                event: ScheduledEvent::Sequence(Event::Control {
                    opcode: 0xBB,
                    value: 150,
                }),
            },
            Scheduled {
                tick: 30,
                track: 0,
                order: 1,
                event: ScheduledEvent::Sequence(Event::Control {
                    opcode: 0xBB,
                    value: 75,
                }),
            },
        ];
        let (events, end) = map_frames(timeline, 60, sample_rate)?;
        let expected = |gba_frames: u128| {
            let frames = gba_frames * u128::from(sample_rate) * 280_896 / 16_777_216;
            (frames as usize + 4) / 8 * 8
        };
        assert_eq!(events[0].frame, 0);
        assert_eq!(events[1].frame, expected(15));
        assert_eq!(end, expected(45));
    }
    Ok(())
}

#[test]
fn duration_cap_and_fade_use_seconds_at_every_output_rate() -> Result<()> {
    for sample_rate in SAMPLE_RATES {
        let rendered = render_fixture_options(
            0,
            8_000,
            127,
            127,
            RenderOptions {
                sample_rate,
                max_seconds: 1,
                fade_seconds: 1,
                ..RenderOptions::default()
            },
        )?;
        assert_eq!(rendered.pcm.len(), sample_rate as usize * 2);
        assert!(rendered.pcm.iter().any(|point| point.unsigned_abs() > 1000));
        assert!(
            rendered.pcm[rendered.pcm.len() - 16..]
                .iter()
                .all(|point| point.unsigned_abs() < 10)
        );
    }
    Ok(())
}

#[test]
fn unsupported_output_rates_fail_before_synthesis() {
    for sample_rate in [0, 8_000, 32_000, 63_071, 192_000, u32::MAX] {
        let error = render_fixture_options(
            0,
            8_000,
            127,
            127,
            RenderOptions {
                sample_rate,
                ..RenderOptions::default()
            },
        )
        .err()
        .expect("unsupported rate must fail");
        assert!(error.to_string().contains("output sample rate"));
    }
}

#[test]
fn an_empty_track_and_wait_only_loop_do_not_prevent_other_tracks_from_rendering() -> Result<()> {
    let zero = Program {
        events: vec![TimedEvent {
            tick: 0,
            event: Event::Fine,
        }],
        ticks: 0,
        loop_start: None,
        loop_event_start: None,
    };
    let waiting = Program {
        events: Vec::new(),
        ticks: 1,
        loop_start: Some(0),
        loop_event_start: Some(0),
    };
    let long = Program {
        events: vec![note(0, 60, 1)],
        ticks: 1_000_000,
        loop_start: None,
        loop_event_start: None,
    };
    let (timeline, end) = build_timeline(&[zero, waiting, long], 1)?;
    assert_eq!(end, 1_000_000);
    assert_eq!(timeline.len(), 3);
    Ok(())
}

mod session;
