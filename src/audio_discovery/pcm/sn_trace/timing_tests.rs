use super::*;
use zeff_emu_common::audio_trace::{AudioTraceEvent, AudioTraceSource};

#[derive(Clone, Copy)]
struct Profile {
    model: Model,
    clock: u32,
    stereo: bool,
}

impl Profile {
    fn chip(self, rate: u32) -> Chip {
        Chip::new(self.model, self.clock, rate)
    }

    fn write(self, value: u8) -> AudioTraceWrite {
        AudioTraceWrite::Sn76489 {
            port: if matches!(self.model, Model::Ti) {
                0xe0
            } else {
                0x7f
            },
            value,
        }
    }

    fn trace(self, events: Vec<AudioTraceEvent>, end_cycle: u64) -> AudioTrace {
        AudioTrace {
            generation: 1,
            cycle_hz: self.clock,
            cycle_hz_denominator: 1,
            chip: self.model.contract(self.clock, self.stereo),
            timing: if matches!(self.model, Model::Ti) {
                AudioTraceTiming::IoWriteCompletion
            } else {
                AudioTraceTiming::InstructionBoundary
            },
            start: AudioTraceStart::Reset,
            end_cycle,
            events,
            dropped_events: 0,
            invalidated: None,
        }
    }
}

fn profiles() -> [Profile; 5] {
    let ntsc = Sega8VideoStandard::Ntsc.clock_hz_approx();
    let pal = Sega8VideoStandard::Pal.clock_hz_approx();
    [
        Profile {
            model: Model::Sega,
            clock: ntsc,
            stereo: false,
        },
        Profile {
            model: Model::Sega,
            clock: pal,
            stereo: false,
        },
        Profile {
            model: Model::Sega,
            clock: ntsc,
            stereo: true,
        },
        Profile {
            model: Model::Sega,
            clock: pal,
            stereo: true,
        },
        Profile {
            model: Model::Ti,
            clock: zeff_coleco_core::psg::COLECO_PSG_INPUT_CLOCK_HZ,
            stereo: false,
        },
    ]
}

fn sample_cycle(frame: u64, clock: u32, rate: u32) -> u64 {
    ((frame + 1) * u64::from(clock)).div_ceil(u64::from(rate))
}

fn event(cycle: u64, write: AudioTraceWrite) -> AudioTraceEvent {
    AudioTraceEvent {
        cycle,
        pc: 0,
        instruction_source: AudioTraceSource::Unknown,
        write,
    }
}

#[test]
fn native_emission_phase_and_writes_at_sample_boundaries_are_exact() {
    for profile in profiles() {
        for rate in [44_100, 48_000, 63_072, 96_000] {
            let mut chip = profile.chip(rate);
            chip.write(profile.write(0x90));
            let mut emitted = 0;
            let mut samples = Vec::new();
            for cycle in 1..=sample_cycle(15, profile.clock, rate) {
                samples.clear();
                chip.advance(1, &mut samples);
                if !samples.is_empty() {
                    assert_eq!(samples.len(), 2);
                    assert!(samples.iter().all(|sample| *sample != 0.0));
                    assert_eq!(cycle, sample_cycle(emitted, profile.clock, rate));
                    emitted += 1;
                }
                assert_eq!(emitted, cycle * u64::from(rate) / u64::from(profile.clock));
            }
            assert_eq!(emitted, 16);

            let (mut a, mut b) = (profile.clock, rate);
            while b != 0 {
                (a, b) = (b, a % b);
            }
            let exact = u64::from(profile.clock / a);
            assert_eq!(exact * u64::from(rate) % u64::from(profile.clock), 0);
            let fractional = sample_cycle(0, profile.clock, rate);
            assert_ne!(fractional * u64::from(rate) % u64::from(profile.clock), 0);
            for cycle in [
                0,
                fractional - 1,
                fractional,
                fractional + 1,
                exact - 1,
                exact,
                exact + 1,
            ] {
                for starts_muted in [false, true] {
                    let mut chip = profile.chip(rate);
                    if !starts_muted {
                        chip.write(profile.write(0x90));
                    }
                    samples.clear();
                    chip.advance(cycle as u32, &mut samples);
                    let first_affected = cycle * u64::from(rate) / u64::from(profile.clock);
                    assert_eq!(samples.len() as u64, first_affected * 2);
                    assert!(
                        samples
                            .iter()
                            .all(|sample| (*sample == 0.0) == starts_muted)
                    );
                    chip.write(profile.write(if starts_muted { 0x90 } else { 0x9f }));
                    samples.clear();
                    let next = sample_cycle(first_affected, profile.clock, rate);
                    chip.advance((next - cycle) as u32, &mut samples);
                    assert_eq!(samples.len(), 2);
                    assert!(
                        samples
                            .iter()
                            .all(|sample| (*sample != 0.0) == starts_muted)
                    );
                }
            }
        }
    }
}

#[test]
fn every_native_channel_has_immediate_volume_and_routing_mute() {
    for profile in profiles() {
        for rate in [44_100, 48_000, 63_072, 96_000] {
            for channel in 0..4 {
                let mut chip = profile.chip(rate);
                let mut samples = Vec::new();
                chip.write(profile.write(0x90 | (channel << 5)));
                chip.advance(256, &mut samples);
                assert!(!samples.is_empty());
                assert!(samples.iter().all(|sample| *sample != 0.0));
                chip.write(profile.write(0x0f));
                samples.clear();
                chip.advance(256, &mut samples);
                assert!(samples.iter().all(|sample| *sample == 0.0));
                chip.write(profile.write(0x00));
                if profile.stereo {
                    chip.write(AudioTraceWrite::GameGearStereo {
                        port: 6,
                        value: !(0x11 << channel),
                    });
                    samples.clear();
                    chip.advance(256, &mut samples);
                    assert!(samples.iter().all(|sample| *sample == 0.0));
                    for route in [1 << channel, 0x10 << channel] {
                        chip.write(AudioTraceWrite::GameGearStereo {
                            port: 6,
                            value: route,
                        });
                        samples.clear();
                        chip.advance(256, &mut samples);
                        assert!(samples.as_chunks::<2>().0.iter().all(|pair| {
                            (pair[0] != 0.0) == (route & 0xf0 != 0)
                                && (pair[1] != 0.0) == (route & 0x0f != 0)
                        }));
                    }
                    chip.write(profile.write(0x0f));
                    samples.clear();
                    chip.advance(256, &mut samples);
                    assert!(samples.iter().all(|sample| *sample == 0.0));
                }
            }
        }
    }
}

#[test]
fn native_boundary_pcm_matches_trace_chunks_and_reset_without_latency() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for profile in profiles() {
        for rate in [44_100, 48_000, 63_072, 96_000] {
            let at = |frame| sample_cycle(frame, profile.clock, rate);
            let mute = at(3);
            let resume = at(7) - 1;
            let end_cycle = at(31);
            let mut events = vec![
                event(0, profile.write(0x90)),
                event(mute, profile.write(0x9f)),
                event(resume, profile.write(0x9f)),
                event(resume, profile.write(0x00)),
            ];
            if profile.stereo {
                events.extend([
                    event(
                        at(11) + 1,
                        AudioTraceWrite::GameGearStereo { port: 6, value: 0 },
                    ),
                    event(
                        at(15),
                        AudioTraceWrite::GameGearStereo {
                            port: 6,
                            value: 0x11,
                        },
                    ),
                ]);
            }
            events.push(event(end_cycle, profile.write(0x9f)));
            let trace = profile.trace(events, end_cycle);
            let mut chip = profile.chip(rate);
            let mut floats = Vec::new();
            let mut cycle = 0;
            for event in &trace.events {
                chip.advance((event.cycle - cycle) as u32, &mut floats);
                chip.write(event.write);
                cycle = event.cycle;
            }
            assert_eq!(floats.len(), 64);
            for (index, pair) in floats.as_chunks::<2>().0.iter().enumerate() {
                let muted =
                    (4..7).contains(&index) || (profile.stereo && (12..16).contains(&index));
                assert!(pair.iter().all(|sample| (*sample == 0.0) == muted));
            }
            let expected: Vec<_> = floats.iter().map(|sample| sample.to_bits()).collect();
            let projected: Vec<_> = floats
                .iter()
                .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
                .collect();
            let options = RenderOptions {
                sample_rate: rate,
                max_seconds: 1,
                ..Default::default()
            };
            let mut session = SnTraceSession::new(trace.clone(), options, &cancel)?;
            for size in [2, 14, 38, 2048] {
                session.reset()?;
                let mut buffer = vec![0; size];
                let mut bits = Vec::new();
                let mut pcm = Vec::new();
                loop {
                    let count = session.read(&mut buffer, &cancel)?;
                    if count == 0 {
                        break;
                    }
                    pcm.extend_from_slice(&buffer[..count]);
                    bits.extend(session.floats.iter().map(|sample| sample.to_bits()));
                }
                assert_eq!(bits, expected);
                assert_eq!(pcm, projected);
            }
            let mut fresh = SnTraceSession::new(trace, options, &cancel)?;
            let mut buffer = [0; 128];
            assert_eq!(fresh.read(&mut buffer, &cancel)?, projected.len());
            assert_eq!(&buffer[..projected.len()], projected);
        }
    }
    Ok(())
}
