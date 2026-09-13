use super::*;
use std::io::Read;
use zeff_emu_common::system::System;

mod gb_banked;
mod gb_musyx;
mod gb_tose;
mod native_extensions;
mod nes_queue;
mod sega;

fn input(system: System, bytes: Vec<u8>) -> ScanInput {
    ScanInput {
        cdda: None,
        system: Some(system),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "multi-engine-export-test",
        display_name: None,
    }
}

#[test]
fn embedded_formats_preserve_the_validated_extent_for_every_cartridge_system() -> Result<()> {
    for (module, format) in [
        (
            super::super::test_support::tracker::xm_fixture(),
            SongFormat::Xm,
        ),
        (
            super::super::test_support::tracker::mod_fixture(),
            SongFormat::Mod,
        ),
        (
            super::super::test_support::tracker::s3m_fixture(),
            SongFormat::S3m,
        ),
        (
            super::super::test_support::tracker::it_fixture(),
            SongFormat::It,
        ),
    ] {
        for system in [
            System::Gba,
            System::Gb,
            System::Nes,
            System::Coleco,
            System::Pce,
            System::Ws,
            System::Sms,
            System::Gg,
            System::Sg,
        ] {
            let mut bytes = vec![0xfe; 37];
            bytes.extend_from_slice(&module);
            bytes.extend_from_slice(&[0xab; 31]);
            let input = input(system, bytes);
            let manifest = input.analyze(Default::default(), &AtomicBool::new(false));
            let id = manifest.scan.song_at_offset(37)?;
            assert_eq!(id, SongId::Module(0));
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("original.module");
            let request =
                || SongExportRequest::prepare(&input, &manifest, id, format, Default::default());
            request()?.write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))?;
            assert_eq!(std::fs::read(&path)?, module);
            assert!(
                request()?
                    .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
                    .is_err()
            );
            let cancelled = directory.path().join("cancelled.module");
            assert!(
                request()?
                    .write_new(&cancelled, &AtomicBool::new(true), &AtomicU32::new(0))
                    .is_err()
            );
            assert!(!cancelled.exists());
            assert_eq!(std::fs::read(path)?, module);
            let pack = directory.path().join("pack.zip");
            SongExportRequest::prepare(
                &input,
                &manifest,
                id,
                SongFormat::TrackerPack,
                Default::default(),
            )?
            .write_new(&pack, &AtomicBool::new(false), &AtomicU32::new(0))?;
            let mut archive = zip::ZipArchive::new(std::fs::File::open(pack)?)?;
            let mut packed = Vec::new();
            archive
                .by_name(&format!("song.{}", format.info().extension))?
                .read_to_end(&mut packed)?;
            assert_eq!(packed, module);
            let metadata: Value = serde_json::from_reader(archive.by_name("manifest.json")?)?;
            assert_eq!(metadata["media"]["system"], system.code());
            assert_eq!(metadata["song"]["span"]["offset"], 37);
            assert_eq!(
                metadata["module_sha256"],
                zeff_firmware::sha256_hex(&module)
            );
        }
    }
    Ok(())
}

#[test]
fn gax_projection_and_mapped_originals_share_a_verified_source() -> Result<()> {
    let input = input(System::Gba, super::super::test_support::gax::fixture());
    let manifest = input.analyze(Default::default(), &AtomicBool::new(false));
    assert_eq!(manifest.scan.gax_songs.len(), 1);
    let directory = tempfile::tempdir()?;
    let id = SongId::Gax(0);
    let xm = directory.path().join("song.xm");
    SongExportRequest::prepare(&input, &manifest, id, SongFormat::Xm, Default::default())?
        .write_new(&xm, &AtomicBool::new(false), &AtomicU32::new(0))?;
    let projected = std::fs::read(xm)?;
    let parsed = super::super::scan(
        System::Gba,
        &projected,
        Default::default(),
        &AtomicBool::new(false),
    );
    assert_eq!(parsed.tracker_modules.len(), 1);
    let path = directory.path().join("mapped.zip");
    SongExportRequest::prepare(
        &input,
        &manifest,
        id,
        SongFormat::MappedAssets,
        Default::default(),
    )?
    .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    for span in &manifest.scan.gax_songs[0].mapped_spans {
        let start = span.effective_offset as usize;
        let mut data = Vec::new();
        archive
            .by_name(&format!("source/{start:08x}-{:08x}.bin", span.byte_len))?
            .read_to_end(&mut data)?;
        assert_eq!(data, input.bytes[start..start + span.byte_len as usize]);
    }
    assert!(
        SongExportRequest::prepare(&input, &manifest, id, SongFormat::Midi, Default::default())
            .is_err()
    );
    let mut changed = input.bytes.to_vec();
    changed[0] ^= 1;
    let stale = self::input(System::Gba, changed);
    let path = directory.path().join("stale.xm");
    assert!(
        SongExportRequest::prepare(&stale, &manifest, id, SongFormat::Xm, Default::default())?
            .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
            .is_err()
    );
    assert!(!path.exists());
    Ok(())
}

#[test]
fn cd_catalog_rejects_changed_identity_and_exports_original_pcm() -> Result<()> {
    use super::super::cdda::{CdAudioInput, CdAudioProvenance};
    use zeff_pce_core::hardware::{CdDisc, CdTrack, CdTrackMode};
    let pcm: Vec<u8> = (0..588)
        .flat_map(|_| [17i16.to_le_bytes(), (-19i16).to_le_bytes()].concat())
        .collect();
    let disc = Arc::new(CdDisc::new(vec![
        CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, vec![0; 2048])?,
        CdTrack::from_index1_data(2, 0, None, 1, CdTrackMode::Audio, pcm.clone())?,
    ])?);
    let hash = const_hex::encode(disc.content_hash());
    let audio = CdAudioInput::new(
        Arc::clone(&disc),
        hash.clone(),
        disc.payload_len(),
        hash.clone(),
        CdAudioProvenance {
            source_kind: "synthetic",
            source_media_sha256: "11".repeat(32),
            source_media_len: 1,
            selected_member_path_sha256: None,
            transforms_applied: false,
        },
    )?;
    let input = ScanInput::from_disc(audio, "cd-export-test");
    let mut manifest = input.analyze(Default::default(), &AtomicBool::new(false));
    assert_eq!(manifest.scan.status, super::super::ScanStatus::Complete);
    assert_eq!(manifest.scan.song_count(), 1);
    assert!(manifest.scan.song_at_offset(1).is_err());
    let id = SongId::Cdda(0);
    assert!(manifest.scan.song(id).unwrap().span().is_none());
    let format = SongFormat::Audio(super::super::formats::AudioFormat::Wav);
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("track.wav");
    SongExportRequest::prepare(&input, &manifest, id, format, Default::default())?.write_new(
        &path,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )?;
    let mut wav = hound::WavReader::open(&path)?;
    assert_eq!(wav.spec().sample_rate, 44_100);
    assert_eq!(wav.spec().channels, 2);
    let decoded = wav
        .samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let expected = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect::<Vec<_>>();
    assert_eq!(decoded, expected);
    manifest.disc.as_mut().unwrap().provenance.source_media_len += 1;
    assert!(SongExportRequest::prepare(&input, &manifest, id, format, Default::default()).is_err());
    manifest.disc.as_mut().unwrap().provenance.source_media_len -= 1;
    manifest.scan.cdda_tracks[0].index1_lba += 1;
    assert!(SongExportRequest::prepare(&input, &manifest, id, format, Default::default()).is_err());
    let cancelled = input.analyze(Default::default(), &AtomicBool::new(true));
    assert_eq!(
        cancelled.scan.status,
        super::super::ScanStatus::Incomplete(super::super::ScanStop::Cancelled)
    );
    assert_eq!(cancelled.scan.song_count(), 0);
    Ok(())
}

#[test]
fn standalone_tracker_exports_require_a_matching_standalone_input() {
    use super::super::media::SourceIdentity;
    use super::super::tracker::{EmbeddedFormat, ModuleSource};

    let bytes = super::super::test_support::tracker::xm_fixture();
    let input = ScanInput::standalone_tracker(
        bytes.clone(),
        SourceIdentity {
            kind: "synthetic_xm",
            sha256: zeff_firmware::sha256_hex(&bytes),
            len: bytes.len(),
            container: None,
            selected_member: None,
        },
        EmbeddedFormat::Xm,
        None,
    );
    let mut manifest = input.analyze(Default::default(), &AtomicBool::new(false));
    let id = SongId::Module(0);
    assert!(
        SongExportRequest::prepare(&input, &manifest, id, SongFormat::Xm, Default::default())
            .is_ok()
    );
    manifest.scan.tracker_modules[0].source = ModuleSource::Embedded;
    assert!(
        SongExportRequest::prepare(&input, &manifest, id, SongFormat::Xm, Default::default())
            .is_err()
    );
}

#[test]
fn aas_catalog_preview_and_mapped_exports_keep_the_selected_source() -> Result<()> {
    use super::super::{formats::AudioFormat, pcm::song::PcmSong, preview::PreviewRequest};

    let cancel = AtomicBool::new(false);
    for current in [false, true] {
        let input = Arc::new(input(System::Gba, super::super::aas::fixture_rom(current)));
        let manifest = input.analyze(Default::default(), &cancel);
        assert_eq!(manifest.scan.aas_songs.len(), 2);
        let id = SongId::Aas(1);
        let selected = &manifest.scan.aas_songs[1];
        assert_eq!(
            manifest
                .scan
                .song_at_offset(selected.header.effective_offset)?,
            id
        );
        let song = manifest.scan.song(id).unwrap();
        assert!(PcmSong::can_play(song) && PcmSong::is_native(song));
        for format in [AudioFormat::Wav, AudioFormat::Flac, AudioFormat::Ogg] {
            assert!(song.supports(SongFormat::Audio(format)));
        }
        assert!(song.supports(SongFormat::Gsf));
        assert!(song.supports(SongFormat::MiniGsfPack));
        for format in [SongFormat::Midi, SongFormat::Xm] {
            assert!(!song.supports(format));
        }
        PreviewRequest::prepare_song(&input, &manifest, id, Default::default())?;
        let graph = manifest
            .scan
            .asset_relations(id, Default::default(), &cancel);
        assert_eq!(graph.song, id);
        assert_eq!(graph.media_sha256, manifest.scan.media.sha256);

        let directory = tempfile::tempdir()?;
        let path = directory.path().join("selected.zip");
        SongExportRequest::prepare(
            &input,
            &manifest,
            id,
            SongFormat::MappedAssets,
            Default::default(),
        )?
        .write_new(&path, &cancel, &AtomicU32::new(0))?;
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        for span in &selected.mapped_spans {
            let start = span.effective_offset as usize;
            let mut bytes = Vec::new();
            archive
                .by_name(&format!("source/{start:08x}-{:08x}.bin", span.byte_len))?
                .read_to_end(&mut bytes)?;
            assert_eq!(bytes, input.bytes[start..start + span.byte_len as usize]);
        }
        let metadata: Value = serde_json::from_reader(archive.by_name("manifest.json")?)?;
        assert_eq!(metadata["selection"]["engine"], "aas");
        assert_eq!(metadata["selection"]["song"]["index"], 1);

        let mut changed = input.bytes.to_vec();
        *changed.last_mut().unwrap() ^= 1;
        let stale = self::input(System::Gba, changed);
        let path = directory.path().join("stale.zip");
        assert!(
            SongExportRequest::prepare(
                &stale,
                &manifest,
                id,
                SongFormat::MappedAssets,
                Default::default(),
            )?
            .write_new(&path, &cancel, &AtomicU32::new(0))
            .is_err()
        );
        assert!(!path.exists());
    }
    Ok(())
}

#[test]
fn native_selections_preserve_raw_selectors_and_mapped_sources() -> Result<()> {
    use super::super::{formats::AudioFormat, pcm::song::PcmSong, preview::PreviewRequest};

    let cancel = AtomicBool::new(false);
    for (bytes, id, engine, raw_index) in [
        (
            super::super::descriptor_midi::fixture_rom(),
            SongId::DescriptorMidi(1),
            "descriptor_midi",
            2,
        ),
        (super::super::nsq::fixture_rom(), SongId::Nsq(1), "nsq", 115),
        (
            super::super::gb_native::fixture_rom(),
            SongId::GbNative(0),
            "gb_native",
            0xba,
        ),
        (
            super::super::aas_stream::fixture_rom(),
            SongId::AasStream(1),
            "aas_stream",
            1,
        ),
        (
            zeff_audio_discovery::aas_pcm::fixture_rom(),
            SongId::AasPcm(1),
            "aas_pcm",
            1,
        ),
        (
            super::super::gbass::fixture_rom(),
            SongId::Gbass(0),
            "gbass",
            0,
        ),
        (
            super::super::gbass::fixture_rom_partial(),
            SongId::Gbass(1),
            "gbass",
            1,
        ),
        (
            super::super::nes_native::fixture_rom(),
            SongId::NesNative(2),
            "nes_native",
            2,
        ),
        (
            super::super::radriver::fixture_rom(),
            SongId::Radriver(1),
            "radriver",
            1,
        ),
        (
            super::super::radriver::fixture_rom(),
            SongId::Radriver(3),
            "radriver",
            1,
        ),
    ] {
        let system = match id {
            SongId::NesNative(_) => System::Nes,
            SongId::GbNative(_) => System::Gb,
            _ => System::Gba,
        };
        let input = Arc::new(input(system, bytes));
        let manifest = input.analyze(Default::default(), &cancel);
        let song = manifest.scan.song(id).context("missing native selection")?;
        assert_eq!(
            manifest
                .scan
                .song_at_offset(song.span().unwrap().effective_offset)?,
            id
        );
        assert!(PcmSong::can_play(song) && PcmSong::is_native(song));
        assert_eq!(song.supports(SongFormat::Midi), engine == "descriptor_midi");
        for format in [AudioFormat::Wav, AudioFormat::Flac, AudioFormat::Ogg] {
            assert!(song.supports(SongFormat::Audio(format)));
            SongExportRequest::prepare(
                &input,
                &manifest,
                id,
                SongFormat::Audio(format),
                Default::default(),
            )?;
        }
        PreviewRequest::prepare_song(&input, &manifest, id, Default::default())?;
        let graph = manifest
            .scan
            .asset_relations(id, Default::default(), &cancel);
        assert_eq!(graph.song, id);
        assert_eq!(graph.media_sha256, manifest.scan.media.sha256);
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("selected.zip");
        SongExportRequest::prepare(
            &input,
            &manifest,
            id,
            SongFormat::MappedAssets,
            Default::default(),
        )?
        .write_new(&path, &cancel, &AtomicU32::new(0))?;
        let spans = match song {
            SongRef::DescriptorMidi(song) => &song.mapped_spans,
            SongRef::Nsq(song) => &song.mapped_spans,
            SongRef::Radriver(song) => &song.mapped_spans,
            SongRef::Gbass(song) => &song.mapped_spans,
            SongRef::NesNative(song) => &song.mapped_spans,
            SongRef::GbNative(song) => &song.mapped_spans,
            SongRef::AasStream(song) => &song.mapped_spans,
            SongRef::AasPcm(song) => &song.mapped_spans,
            _ => unreachable!(),
        };
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        for span in spans {
            let start = span.effective_offset as usize;
            let mut bytes = Vec::new();
            archive
                .by_name(&format!("source/{start:08x}-{:08x}.bin", span.byte_len))?
                .read_to_end(&mut bytes)?;
            assert_eq!(bytes, input.bytes[start..start + span.byte_len as usize]);
        }
        let metadata: Value = serde_json::from_reader(archive.by_name("manifest.json")?)?;
        assert_eq!(metadata["selection"]["engine"], engine);
        let selector_field = if matches!(id, SongId::GbNative(_)) {
            "raw_index"
        } else {
            "index"
        };
        assert_eq!(metadata["selection"]["song"][selector_field], raw_index);
        let mut changed = input.bytes.to_vec();
        *changed.last_mut().unwrap() ^= 1;
        let stale = self::input(system, changed);
        let path = directory.path().join("stale.zip");
        assert!(
            SongExportRequest::prepare(
                &stale,
                &manifest,
                id,
                SongFormat::MappedAssets,
                Default::default()
            )?
            .write_new(&path, &cancel, &AtomicU32::new(0))
            .is_err()
        );
        assert!(!path.exists());
    }
    Ok(())
}

#[test]
fn descriptor_midi_export_is_exact_atomic_and_rejects_stale_sources() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let input = input(System::Gba, super::super::descriptor_midi::fixture_rom());
    let manifest = input.analyze(Default::default(), &cancel);
    let id = SongId::DescriptorMidi(1);
    let selected = &manifest.scan.descriptor_midi_songs[1];
    let start = selected.midi.effective_offset as usize;
    let expected = &input.bytes[start..start + selected.midi.byte_len as usize];
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("selected.mid");
    let request =
        || SongExportRequest::prepare(&input, &manifest, id, SongFormat::Midi, Default::default());
    request()?.write_new(&path, &cancel, &AtomicU32::new(0))?;
    assert_eq!(std::fs::read(&path)?, expected);
    assert!(
        request()?
            .write_new(&path, &cancel, &AtomicU32::new(0))
            .is_err()
    );
    let cancelled = directory.path().join("cancelled.mid");
    assert!(
        request()?
            .write_new(&cancelled, &AtomicBool::new(true), &AtomicU32::new(0))
            .is_err()
    );
    assert!(!cancelled.exists());
    let mut changed = input.bytes.to_vec();
    changed[start] ^= 1;
    let stale = self::input(System::Gba, changed);
    let rejected = directory.path().join("stale.mid");
    assert!(
        SongExportRequest::prepare(&stale, &manifest, id, SongFormat::Midi, Default::default())?
            .write_new(&rejected, &cancel, &AtomicU32::new(0))
            .is_err()
    );
    assert!(!rejected.exists());
    Ok(())
}
