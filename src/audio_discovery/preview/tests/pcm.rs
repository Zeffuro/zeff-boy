use super::*;
use crate::audio_discovery::{
    catalog::SongId,
    export::SongExportRequest,
    formats::{AudioFormat, SongFormat},
    test_support,
};

#[test]
fn engine_preview_transport_and_wav_share_the_selected_song() -> anyhow::Result<()> {
    let cancel = AtomicBool::new(false);
    use zeff_emu_common::system::System;
    for (bytes, id, system) in [
        (
            test_support::engine_software::fixture(),
            SongId::EngineSoftware(0),
            System::Gba,
        ),
        (test_support::gax::fixture(), SongId::Gax(0), System::Gba),
        (
            zeff_audio_discovery::gb_native::fixture_rom(),
            SongId::GbNative(0),
            System::Gb,
        ),
        (
            zeff_audio_discovery::gb_music::native::fixture_rom(),
            SongId::Gb(1),
            System::Gb,
        ),
        (
            zeff_audio_discovery::gb_tose::synthetic_rom(),
            SongId::GbTose(0),
            System::Gb,
        ),
        (
            zeff_audio_discovery::gb_quickthunder::synthetic_rom(),
            SongId::GbQuickThunder(0),
            System::Gb,
        ),
        (
            zeff_audio_discovery::nes_tose::synthetic_rom(),
            SongId::NesTose(0),
            System::Nes,
        ),
        (
            zeff_audio_discovery::gb_musyx::synthetic_rom(),
            SongId::GbMusyx(1),
            System::Gb,
        ),
        (
            zeff_audio_discovery::gbass::fixture_rom_started(),
            SongId::Gbass(0),
            System::Gba,
        ),
        (
            zeff_audio_discovery::gbass::fixture_rom_partial(),
            SongId::Gbass(1),
            System::Gba,
        ),
        (
            zeff_audio_discovery::aas_stream::fixture_rom(),
            SongId::AasStream(1),
            System::Gba,
        ),
        (
            zeff_audio_discovery::aas_pcm::fixture_rom(),
            SongId::AasPcm(1),
            System::Gba,
        ),
        (
            zeff_audio_discovery::nes_native::fixture_rom(),
            SongId::NesNative(0),
            System::Nes,
        ),
        (
            zeff_audio_discovery::nes_music::native::fixture_rom(),
            SongId::Nes(8),
            System::Nes,
        ),
        (
            zeff_audio_discovery::nes_native::fixture_rom_pc10(),
            SongId::NesNative(0),
            System::Nes,
        ),
        (
            zeff_audio_discovery::gbass::fixture_rom(),
            SongId::Gbass(0),
            System::Gba,
        ),
        (
            zeff_audio_discovery::gbass::fixture_rom_irq(),
            SongId::Gbass(0),
            System::Gba,
        ),
        (
            zeff_audio_discovery::sega_psg::fixture_rom(),
            SongId::SegaPsg(0),
            System::Sms,
        ),
    ] {
        let input = Arc::new(ScanInput {
            cdda: None,
            system: Some(system),
            standalone_audio: None,
            bytes: bytes.into(),
            provenance: None,
            analysis_profile: "projected-engine-preview-test",
            display_name: None,
        });
        let manifest = input.analyze(Default::default(), &cancel);
        let song = manifest.scan.song(id).expect("fixture is discovered");
        assert!(song.supports(SongFormat::Audio(AudioFormat::Wav)));
        let options = RenderOptions {
            max_seconds: 1,
            sample_rate: 48_000,
            ..Default::default()
        };
        let request = || PreviewRequest::prepare_song(&input, &manifest, id, options);
        let mut renderer = request()?.renderer(48_000, &cancel)?;
        let mut expected = vec![0; renderer.duration_frames() * 2];
        assert_eq!(renderer.read(&mut expected, &cancel)?, expected.len());
        assert!(expected.iter().any(|sample| *sample != 0));
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("selected.wav");
        SongExportRequest::prepare(
            &input,
            &manifest,
            id,
            SongFormat::Audio(AudioFormat::Wav),
            options,
        )?
        .write_new(&path, &cancel, &AtomicU32::new(0))?;
        let mut wav = hound::WavReader::open(path)?;
        assert_eq!(
            wav.samples::<i16>().collect::<Result<Vec<_>, _>>()?,
            expected
        );
        let expected = expected
            .iter()
            .map(|sample| f32::from(*sample) / 32768.0)
            .collect::<Vec<_>>();
        let mut player = PreviewPlayer::default();
        player.set_volume(100);
        let receiver = player.start_captured(request()?);
        let mut callback = receiver.recv_timeout(Duration::from_secs(10))?;
        assert_eq!(
            consumed_pcm(&mut player, &mut callback, 1024),
            expected[..2048]
        );
        player.set_playing(false);
        player.seek(8000);
        wait(&mut player, |player| {
            player.snapshot().is_some_and(|state| !state.preparing)
        });
        assert_eq!(player.snapshot().unwrap().position, 8000);
        player.set_playing(true);
        assert_eq!(
            consumed_pcm(&mut player, &mut callback, 1024),
            expected[16000..18048]
        );
        player.seek(0);
        assert_eq!(
            consumed_pcm(&mut player, &mut callback, 1024),
            expected[..2048]
        );
        player.stop();
        wait(&mut player, |player| !player.is_pending());
    }
    Ok(())
}
