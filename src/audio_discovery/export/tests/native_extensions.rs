use super::*;
use crate::audio_discovery::{formats::AudioFormat, pcm::song::PcmSong, relations::GraphStatus};

#[test]
fn additional_native_drivers_preserve_selection_assets_and_pcm() -> Result<()> {
    for (system, bytes, id, engine) in [
        (
            System::Gb,
            zeff_audio_discovery::gb_native::timer_fixture_rom(),
            SongId::GbNative(0),
            "gb_native",
        ),
        (
            System::Gb,
            zeff_audio_discovery::gb_native::timer_small_fixture_rom(),
            SongId::GbNative(0),
            "gb_native",
        ),
        (
            System::Nes,
            zeff_audio_discovery::nes_native::fixture_rom_presets(),
            SongId::NesNative(0),
            "nes_native",
        ),
        (
            System::Nes,
            zeff_audio_discovery::nes_native::fixture_rom_presets_cnrom(),
            SongId::NesNative(0),
            "nes_native",
        ),
        (
            System::Gb,
            zeff_audio_discovery::gb_carillon::synthetic_rom(),
            SongId::GbCarillon(0),
            "gb_carillon",
        ),
        (
            System::Gb,
            zeff_audio_discovery::gb_carillon::synthetic_rom_alternate(),
            SongId::GbCarillon(0),
            "gb_carillon",
        ),
        (
            System::Gb,
            zeff_audio_discovery::gb_ghx::synthetic_rom(),
            SongId::GbGhx(0),
            "gb_ghx",
        ),
        (
            System::Gb,
            zeff_audio_discovery::gb_sound_system::synthetic_rom(),
            SongId::GbSoundSystem(0),
            "gb_sound_system",
        ),
        (
            System::Gb,
            zeff_audio_discovery::gb_sound_system::synthetic_rom_with_hardware(
                zeff_audio_discovery::gb_sound_system::GbSoundSystemHardware::CgbNormal,
            ),
            SongId::GbSoundSystem(0),
            "gb_sound_system",
        ),
        (
            System::Ws,
            zeff_audio_discovery::ws_tose::synthetic_rom(),
            SongId::WsTose(0),
            "ws_tose",
        ),
        (
            System::Ws,
            zeff_audio_discovery::ws_tose::synthetic_legacy_rom(),
            SongId::WsTose(0),
            "ws_tose",
        ),
        (
            System::Gb,
            zeff_audio_discovery::gb_quickthunder::synthetic_rom(),
            SongId::GbQuickThunder(0),
            "gb_quickthunder",
        ),
        (
            System::Gb,
            zeff_audio_discovery::gb_quickthunder::synthetic_rom_rocket(0x97),
            SongId::GbQuickThunder(0),
            "gb_quickthunder",
        ),
        (
            System::Gb,
            zeff_audio_discovery::gb_quickthunder::synthetic_rom_rocket(0x99),
            SongId::GbQuickThunder(0),
            "gb_quickthunder",
        ),
        (
            System::Nes,
            zeff_audio_discovery::nes_tose::synthetic_rom(),
            SongId::NesTose(0),
            "nes_tose",
        ),
        (
            System::Nes,
            zeff_audio_discovery::nes_tose::synthetic_fcg_rom(),
            SongId::NesTose(0),
            "nes_tose",
        ),
    ] {
        verify(system, bytes, id, engine)?;
    }
    for profile in 0..17 {
        verify(
            System::Nes,
            zeff_audio_discovery::nes_tose::synthetic_closed_rom(profile),
            SongId::NesTose(0),
            "nes_tose",
        )?;
    }
    Ok(())
}

#[test]
fn timer_effect_selections_export_and_revalidate_for_both_rom_sizes() -> Result<()> {
    for bytes in [
        zeff_audio_discovery::gb_native::timer_fixture_rom(),
        zeff_audio_discovery::gb_native::timer_small_fixture_rom(),
    ] {
        for index in [2, 3] {
            verify(
                System::Gb,
                bytes.clone(),
                SongId::GbNative(index),
                "gb_native",
            )?;
        }
    }
    Ok(())
}

fn verify(system: System, bytes: Vec<u8>, id: SongId, engine: &str) -> Result<()> {
    let cancel = AtomicBool::new(false);
    let input = input(system, bytes);
    let mut manifest = input.analyze(Default::default(), &cancel);
    let selected = manifest.scan.song(id).unwrap();
    if matches!(id, SongId::GbSoundSystem(_)) {
        assert_eq!(
            selected.classification().role,
            zeff_audio_discovery::classification::AudioRole::Music
        );
    }
    if matches!(id, SongId::GbCarillon(_)) {
        assert_eq!(
            selected.classification().role,
            zeff_audio_discovery::classification::AudioRole::Unknown
        );
    }
    assert!(PcmSong::can_play(selected) && PcmSong::is_native(selected));
    let expected_address = match selected {
        SongRef::GbNative(song) => Some(song.table_entry.canonical_cpu_address),
        SongRef::NesNative(song) => Some(song.table_entry.canonical_cpu_address),
        _ => None,
    };
    assert_eq!(
        selected.span().unwrap().canonical_cpu_address,
        expected_address
    );
    for format in [SongFormat::Midi, SongFormat::Gbs, SongFormat::Nsf] {
        assert!(!selected.supports(format));
    }
    let graph = manifest
        .scan
        .asset_relations(id, Default::default(), &cancel);
    assert_eq!(graph.status, GraphStatus::Complete);
    let directory = tempfile::tempdir()?;
    let options = RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..Default::default()
    };
    let path = directory.path().join("assets.zip");
    SongExportRequest::prepare(&input, &manifest, id, SongFormat::MappedAssets, options)?
        .write_new(&path, &cancel, &AtomicU32::new(0))?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let spans = match id {
        SongId::GbNative(index) => &manifest.scan.gb_native_songs[index].mapped_spans,
        SongId::NesNative(index) => &manifest.scan.nes_native_songs[index].mapped_spans,
        SongId::GbCarillon(index) => &manifest.scan.gb_carillon_songs[index].mapped_spans,
        SongId::GbGhx(index) => &manifest.scan.gb_ghx_songs[index].mapped_spans,
        SongId::GbSoundSystem(index) => &manifest.scan.gb_sound_system_songs[index].mapped_spans,
        SongId::WsTose(index) => &manifest.scan.ws_tose_songs[index].mapped_spans,
        SongId::GbQuickThunder(index) => &manifest.scan.gb_quickthunder_songs[index].mapped_spans,
        SongId::NesTose(index) => &manifest.scan.nes_tose_songs[index].mapped_spans,
        _ => unreachable!(),
    };
    for span in spans {
        let name = format!(
            "source/{:08x}-{:08x}.bin",
            span.effective_offset, span.byte_len
        );
        let mut bytes = Vec::new();
        archive.by_name(&name)?.read_to_end(&mut bytes)?;
        assert_eq!(
            bytes,
            input.bytes
                [span.effective_offset as usize..(span.effective_offset + span.byte_len) as usize]
        );
    }
    let metadata: Value = serde_json::from_reader(archive.by_name("manifest.json")?)?;
    assert_eq!(metadata["selection"]["engine"], engine);
    assert_eq!(
        metadata["classification"],
        serde_json::to_value(manifest.classification(selected))?
    );
    assert_eq!(serde_json::to_value(id)?["engine"], engine);
    let mut session =
        PcmSong::from_ref(selected)
            .unwrap()
            .session(&input.bytes, options, &cancel)?;
    assert!(session.read(&mut [0; 64], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    let mut expected = vec![0; session.duration_frames() * 2];
    assert_eq!(session.read(&mut expected, &cancel)?, 88_200);
    assert!(expected.iter().any(|&v| v != 0));
    session.reset()?;
    let mut replay = Vec::new();
    let mut chunk = [0; 258];
    loop {
        let count = session.read(&mut chunk, &cancel)?;
        if count == 0 {
            break;
        }
        replay.extend_from_slice(&chunk[..count]);
    }
    assert_eq!(replay, expected);
    session.set_track_mask(0)?;
    session.reset()?;
    session.read(&mut chunk, &cancel)?;
    assert!(chunk.iter().all(|&v| v == 0));
    assert!(session.set_track_mask(2).is_err());
    for format in [AudioFormat::Wav, AudioFormat::Flac] {
        let path = directory
            .path()
            .join(format!("song.{}", format.extension()));
        SongExportRequest::prepare(&input, &manifest, id, SongFormat::Audio(format), options)?
            .write_new(&path, &cancel, &AtomicU32::new(0))?;
        let actual = if format == AudioFormat::Wav {
            hound::WavReader::open(path)?
                .samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            claxon::FlacReader::open(path)?
                .samples()
                .map(|v| v.map(|v| v as i16))
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        assert_eq!(actual, expected);
    }
    let spans = match id {
        SongId::GbNative(index) => &mut manifest.scan.gb_native_songs[index].mapped_spans,
        SongId::NesNative(index) => &mut manifest.scan.nes_native_songs[index].mapped_spans,
        SongId::GbCarillon(index) => &mut manifest.scan.gb_carillon_songs[index].mapped_spans,
        SongId::GbGhx(index) => &mut manifest.scan.gb_ghx_songs[index].mapped_spans,
        SongId::GbSoundSystem(index) => {
            &mut manifest.scan.gb_sound_system_songs[index].mapped_spans
        }
        SongId::WsTose(index) => &mut manifest.scan.ws_tose_songs[index].mapped_spans,
        SongId::GbQuickThunder(index) => {
            &mut manifest.scan.gb_quickthunder_songs[index].mapped_spans
        }
        SongId::NesTose(index) => &mut manifest.scan.nes_tose_songs[index].mapped_spans,
        _ => unreachable!(),
    };
    spans[0].effective_offset += 1;
    let path = directory.path().join("forged.zip");
    assert!(
        SongExportRequest::prepare(&input, &manifest, id, SongFormat::MappedAssets, options)?
            .write_new(&path, &cancel, &AtomicU32::new(0))
            .is_err()
    );
    assert!(!path.exists());
    Ok(())
}
