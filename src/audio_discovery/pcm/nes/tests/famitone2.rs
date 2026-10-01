use super::*;

#[test]
fn both_declared_cues_render_without_running_the_original_foreground() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for sample_rate in [44_100, 48_000] {
        let mut prior_layout = None;
        for relocated in [false, true] {
            let bytes = zeff_audio_discovery::nes_native::fixture_rom_famitone2(relocated);
            let report = zeff_audio_discovery::scan(
                zeff_emu_common::system::System::Nes,
                &bytes,
                Default::default(),
                &cancel,
            );
            assert_eq!(report.nes_native_songs.len(), 2);
            let mut recordings = Vec::new();
            for song in &report.nes_native_songs {
                let prepared =
                    zeff_audio_discovery::nes_native::prepare_rom(&bytes, song, &cancel)?;
                let mut session = NesSession::new(
                    prepared,
                    RenderOptions {
                        sample_rate,
                        ..options()
                    },
                    song.warnings.clone(),
                    &cancel,
                )?;
                let pcm = render(&mut session, 258)?;
                assert_eq!(pcm.len(), sample_rate as usize * 2);
                assert!(pcm.iter().any(|&sample| sample != 0));
                assert!(pcm[pcm.len() / 2..].iter().any(|&sample| sample != 0));
                session.reset()?;
                assert_eq!(render(&mut session, 4096)?, pcm);
                session.reset()?;
                session.set_track_mask(0)?;
                assert!(
                    render(&mut session, 2048)?
                        .iter()
                        .all(|&sample| sample == 0)
                );
                recordings.push(pcm);
            }
            assert_ne!(recordings[0], recordings[1]);
            if let Some(previous) = &prior_layout {
                assert_eq!(&recordings, previous);
            } else {
                prior_layout = Some(recordings);
            }
        }
    }
    Ok(())
}

#[test]
fn four_channel_cues_have_independent_triangle_and_noise_output() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for sample_rate in [44_100, 48_000] {
        let mut previous = None;
        for relocated in [false, true] {
            let bytes =
                zeff_audio_discovery::nes_native::fixture_rom_famitone2_four_channel(relocated);
            let report = zeff_audio_discovery::scan(
                zeff_emu_common::system::System::Nes,
                &bytes,
                Default::default(),
                &cancel,
            );
            assert_eq!(report.nes_native_songs.len(), 2);
            let mut recordings = Vec::new();
            for song in &report.nes_native_songs {
                let mut session = NesSession::new(
                    zeff_audio_discovery::nes_native::prepare_rom(&bytes, song, &cancel)?,
                    RenderOptions {
                        sample_rate,
                        ..options()
                    },
                    song.warnings.clone(),
                    &cancel,
                )?;
                for mask in [4, 8, 15, 0] {
                    session.reset()?;
                    session
                        .emulator
                        .set_apu_channel_mutes(std::array::from_fn(|channel| {
                            mask & (1 << channel) == 0
                        }));
                    let pcm = render(&mut session, 258)?;
                    let tail = &pcm[pcm.len() / 2..];
                    if mask == 0 {
                        assert!(pcm.iter().all(|&sample| sample == 0));
                    } else {
                        assert!(tail.iter().min() < tail.iter().max());
                    }
                    session.reset()?;
                    session
                        .emulator
                        .set_apu_channel_mutes(std::array::from_fn(|channel| {
                            mask & (1 << channel) == 0
                        }));
                    assert_eq!(render(&mut session, 4096)?, pcm);
                    recordings.push(pcm);
                }
            }
            if let Some(prior) = &previous {
                assert_eq!(&recordings, prior);
            } else {
                previous = Some(recordings);
            }
        }
    }
    Ok(())
}
