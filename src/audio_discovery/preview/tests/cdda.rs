use super::*;
use crate::audio_discovery::{catalog::SongId, test_support::cdda_fixture};

#[test]
fn cd_preview_transport_matches_source_and_seeks_to_exact_frames() -> anyhow::Result<()> {
    let (input, manifest, source_pcm) = cdda_fixture()?;
    let make_request = || {
        PreviewRequest::prepare_song(&input, &manifest, SongId::Cdda(0), RenderOptions::default())
    };
    let cancel = AtomicBool::new(false);
    let mut original = make_request()?.renderer(44_100, &cancel)?;
    let mut original_pcm = vec![0; source_pcm.len()];
    assert_eq!(
        original.read(&mut original_pcm, &cancel)?,
        original_pcm.len()
    );
    assert_eq!(original_pcm, source_pcm);
    let mut renderer = make_request()?.renderer(48_000, &cancel)?;
    let mut expected = vec![0; renderer.duration_frames() * 2];
    assert_eq!(renderer.read(&mut expected, &cancel)?, expected.len());
    let expected = expected
        .iter()
        .map(|sample| f32::from(*sample) / 32768.0)
        .collect::<Vec<_>>();
    let mut player = PreviewPlayer::default();
    player.set_volume(100);
    let receiver = player.start_captured(make_request()?);
    let mut callback = receiver.recv_timeout(Duration::from_secs(10))?;
    assert_eq!(
        consumed_pcm(&mut player, &mut callback, 1024),
        expected[..2048]
    );
    player.set_playing(false);
    for target in [1, 4097, 588, expected.len() / 2 - 1] {
        player.seek(target);
        wait(&mut player, |player| {
            player.snapshot().is_some_and(|s| !s.preparing)
        });
        let mut silence = [1.0; 2];
        callback.fill(&mut silence, 2);
        assert_eq!(silence, [0.0; 2]);
        assert_eq!(player.snapshot().unwrap().position, target);
        player.set_playing(true);
        assert_eq!(
            consumed_pcm(&mut player, &mut callback, 1),
            expected[target * 2..target * 2 + 2]
        );
        player.set_playing(false);
    }
    player.seek(0);
    player.set_playing(true);
    assert_eq!(
        consumed_pcm(&mut player, &mut callback, 1024),
        expected[..2048]
    );
    player.stop();
    let mut silence = [1.0; 16];
    callback.fill(&mut silence, 2);
    assert_eq!(silence, [0.0; 16]);
    wait(&mut player, |player| !player.is_pending());
    assert_eq!(
        input.analyze(Default::default(), &cancel).scan.cdda_tracks,
        manifest.scan.cdda_tracks
    );
    Ok(())
}

#[test]
fn cd_preview_and_export_reject_mismatched_manifest_identity() -> anyhow::Result<()> {
    use crate::audio_discovery::{
        export::SongExportRequest,
        formats::{AudioFormat, SongFormat},
    };
    for field in 0..7 {
        let (mut input, mut manifest, _) = cdda_fixture()?;
        match field {
            0 => manifest.analysis_profile = "another-profile",
            1 => manifest.scan.media.sha256 = Some("wrong hash".into()),
            2 => manifest.disc.as_mut().unwrap().effective_disc_sha256 = "wrong hash".into(),
            3 => {
                manifest
                    .disc
                    .as_mut()
                    .unwrap()
                    .provenance
                    .source_media_sha256 = "wrong source".into()
            }
            4 => manifest.scan.cdda_tracks[0].pcm_frames -= 588,
            5 => manifest.scan.cdda_tracks[0].pregap_start_lba = None,
            6 => Arc::get_mut(&mut input).unwrap().bytes = vec![1].into(),
            _ => unreachable!(),
        }
        assert!(
            PreviewRequest::prepare_song(
                &input,
                &manifest,
                SongId::Cdda(0),
                RenderOptions::default()
            )
            .is_err()
        );
        assert!(
            SongExportRequest::prepare(
                &input,
                &manifest,
                SongId::Cdda(0),
                SongFormat::Audio(AudioFormat::Wav),
                RenderOptions::default()
            )
            .is_err()
        );
    }
    Ok(())
}
