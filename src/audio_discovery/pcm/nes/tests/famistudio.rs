use super::*;

#[test]
fn declared_cues_preserve_reset_and_relocation_output() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for sample_rate in [44_100, 48_000] {
        let mut prior = None;
        for relocated in [false, true] {
            let bytes = zeff_audio_discovery::nes_native::fixture_rom_famistudio(relocated);
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
                let pcm = render(&mut session, 258)?;
                assert_eq!(pcm.len(), sample_rate as usize * 2);
                let tail = &pcm[pcm.len() / 2..];
                assert!(tail.iter().min() < tail.iter().max());
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
            if let Some(previous) = &prior {
                assert_eq!(&recordings, previous);
            } else {
                prior = Some(recordings);
            }
        }
    }
    Ok(())
}
