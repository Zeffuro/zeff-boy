use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32},
};

use anyhow::Result;
use zeff_audio_discovery::{
    ScanLimits, scan, scan_standalone_tracker,
    tracker::{EmbeddedFormat, EmbeddedModule, ModuleSource},
};
use zeff_emu_common::system::System;

use super::{PcmSession, tracker::TrackerSession};
use crate::audio_discovery::{
    catalog::SongId,
    export::SongExportRequest,
    formats::{AudioFormat, SongFormat},
    media::{ScanInput, StandaloneFormat},
    preview::PreviewRequest,
    render::RenderOptions,
};

fn options() -> RenderOptions {
    RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..Default::default()
    }
}

fn module_bytes() -> Vec<u8> {
    let mut bytes = crate::audio_discovery::test_support::tracker::mod_fixture();
    let sample = 1084 + 64 * 4 * 4;
    bytes.resize(sample + 64, 0);
    bytes[42..44].copy_from_slice(&32u16.to_be_bytes());
    bytes[46..48].copy_from_slice(&0u16.to_be_bytes());
    bytes[48..50].copy_from_slice(&32u16.to_be_bytes());
    bytes[1084..1088].copy_from_slice(&[0x01, 0xac, 0x1e, 0x60]);
    bytes[1100..1104].copy_from_slice(&[0, 0, 0x0e, 0x61]);
    for (index, value) in bytes[sample..].iter_mut().enumerate() {
        *value = if index < 32 { 96 } else { 160 };
    }
    bytes
}

fn module(bytes: &[u8]) -> EmbeddedModule {
    let report = scan(
        System::Gba,
        bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    assert_eq!(report.tracker_modules.len(), 1);
    let module = report.tracker_modules[0].clone();
    assert!(module.mod_playback);
    module
}

fn read_all(session: &mut dyn PcmSession, block: usize) -> Result<Vec<i16>> {
    let cancel = AtomicBool::new(false);
    let mut pcm = Vec::new();
    let mut chunk = vec![0; block];
    while session.position_frames() < session.duration_frames() {
        let count = session.read(&mut chunk, &cancel)?;
        assert!(count > 0 && count.is_multiple_of(2));
        pcm.extend_from_slice(&chunk[..count]);
    }
    assert_eq!(session.read(&mut chunk, &cancel)?, 0);
    Ok(pcm)
}

fn input(bytes: Vec<u8>) -> Arc<ScanInput> {
    Arc::new(ScanInput {
        cdda: None,
        system: Some(System::Gba),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "tracker-mod-pcm-test",
        display_name: None,
    })
}

fn standalone_input(bytes: Vec<u8>) -> Arc<ScanInput> {
    Arc::new(ScanInput {
        cdda: None,
        system: None,
        standalone_audio: Some(StandaloneFormat::Tracker(EmbeddedFormat::Mod)),
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "tracker-mod-pcm-test",
        display_name: None,
    })
}

fn pcm(bytes: &[u8], descriptor: &EmbeddedModule) -> Result<Vec<i16>> {
    let mut session =
        TrackerSession::from_mod(bytes, descriptor, options(), &AtomicBool::new(false))?;
    read_all(&mut session, 512)
}

#[test]
fn mod_session_reset_chunking_masks_and_seek_are_exact() -> Result<()> {
    let bytes = module_bytes();
    let descriptor = module(&bytes);
    let mut session =
        TrackerSession::from_mod(&bytes, &descriptor, options(), &AtomicBool::new(false))?;
    assert_eq!(
        (session.track_count(), session.duration_frames()),
        (4, 44_100)
    );
    let expected = read_all(&mut session, 2048)?;
    assert_eq!(expected.len(), 88_200);
    assert!(expected.iter().any(|sample| *sample != 0));

    session.reset()?;
    assert_eq!(read_all(&mut session, 514)?, expected);
    session.reset()?;
    let mut prefix = [0; 1234];
    assert_eq!(
        session.read(&mut prefix, &AtomicBool::new(false))?,
        prefix.len()
    );
    assert_eq!(read_all(&mut session, 202)?, expected[prefix.len()..]);

    session.reset()?;
    session.set_track_mask(0)?;
    session.reset()?;
    assert!(
        read_all(&mut session, 512)?
            .iter()
            .all(|sample| *sample == 0)
    );
    assert!(session.set_track_mask(0b1_0000).is_err());
    Ok(())
}

#[test]
fn mod_descriptors_keep_their_exact_embedded_and_standalone_spans() -> Result<()> {
    let source = module_bytes();
    let base = module(&source);
    let expected = pcm(&source, &base)?;
    let suffix = vec![0; 1024];
    let mut embedded = vec![0xa5; 23];
    embedded.extend_from_slice(&source);
    embedded.extend_from_slice(&suffix);
    let embedded_module = module(&embedded);
    assert_eq!(embedded_module.span.offset, 23);
    assert_eq!(embedded_module.span.byte_len as usize, source.len());
    assert_eq!(embedded_module.channels, 4);
    assert_eq!(embedded_module.source, ModuleSource::Embedded);
    assert!(embedded_module.mod_playback);
    assert_eq!(pcm(&embedded, &embedded_module)?, expected);

    let mut standalone = source.clone();
    standalone.extend_from_slice(&suffix);
    let report = scan_standalone_tracker(
        &standalone,
        EmbeddedFormat::Mod,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    assert_eq!(report.tracker_modules.len(), 1);
    let standalone_module = &report.tracker_modules[0];
    assert!(standalone_module.mod_playback);
    assert_eq!(standalone_module.channels, 4);
    assert_eq!(standalone_module.span.byte_len as usize, source.len());
    assert_eq!(
        standalone_module.source,
        ModuleSource::Standalone {
            trailing_bytes: suffix.len() as u32,
        }
    );
    assert_eq!(pcm(&standalone, standalone_module)?, expected);
    Ok(())
}

#[test]
fn mod_session_rejects_stale_forged_truncated_and_cancelled_sources() {
    let bytes = module_bytes();
    let descriptor = module(&bytes);
    let mut stale = descriptor.clone();
    stale.span.byte_len -= 1;
    assert!(TrackerSession::from_mod(&bytes, &stale, options(), &AtomicBool::new(false)).is_err());

    let mut unsupported = bytes.clone();
    let effect = 1084 + (4 + 1) * 4;
    unsupported[effect + 2] = (unsupported[effect + 2] & 0xf0) | 0x0e;
    unsupported[effect + 3] = 0xe1;
    let mut forged = scan(
        System::Gba,
        &unsupported,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .tracker_modules
    .remove(0);
    assert!(!forged.mod_playback);
    forged.mod_playback = true;
    assert!(
        TrackerSession::from_mod(&unsupported, &forged, options(), &AtomicBool::new(false))
            .is_err()
    );
    assert!(
        TrackerSession::from_mod(
            &bytes[..bytes.len() - 1],
            &descriptor,
            options(),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(
        TrackerSession::from_mod(&bytes, &descriptor, options(), &AtomicBool::new(true)).is_err()
    );
}

#[test]
fn mod_source_type_mismatches_fail_before_preview_or_export() {
    let source = module_bytes();
    let mut embedded = vec![0xa5; 23];
    embedded.extend_from_slice(&source);
    let embedded = input(embedded);
    let cancel = AtomicBool::new(false);
    let mut manifest = embedded.analyze(Default::default(), &cancel);
    manifest.scan.tracker_modules[0].source = ModuleSource::Standalone { trailing_bytes: 0 };
    assert!(
        PreviewRequest::prepare_song(&embedded, &manifest, SongId::Module(0), options()).is_err()
    );
    assert!(
        SongExportRequest::prepare(
            &embedded,
            &manifest,
            SongId::Module(0),
            SongFormat::Audio(AudioFormat::Wav),
            options(),
        )
        .is_err()
    );

    let standalone = standalone_input(source);
    let mut manifest = standalone.analyze(Default::default(), &cancel);
    manifest.scan.tracker_modules[0].source = ModuleSource::Embedded;
    assert!(
        PreviewRequest::prepare_song(&standalone, &manifest, SongId::Module(0), options()).is_err()
    );
    assert!(
        SongExportRequest::prepare(
            &standalone,
            &manifest,
            SongId::Module(0),
            SongFormat::Audio(AudioFormat::Wav),
            options(),
        )
        .is_err()
    );
}

#[test]
fn mod_wav_matches_preview_and_never_replaces_output() -> Result<()> {
    let source = module_bytes();
    let mut bytes = vec![0x5a; 19];
    bytes.extend_from_slice(&source);
    let input = input(bytes);
    let cancel = AtomicBool::new(false);
    let manifest = input.analyze(Default::default(), &cancel);
    let id = SongId::Module(0);
    assert!(manifest.scan.tracker_modules[0].mod_playback);
    let mut preview = PreviewRequest::prepare_song(&input, &manifest, id, options())?
        .renderer(44_100, &cancel)?;
    let expected = read_all(&mut preview, 258)?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("module.wav");
    SongExportRequest::prepare(
        &input,
        &manifest,
        id,
        SongFormat::Audio(AudioFormat::Wav),
        options(),
    )?
    .write_new(&path, &cancel, &AtomicU32::new(0))?;
    let mut wav = hound::WavReader::open(&path)?;
    assert_eq!(
        wav.samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        expected
    );
    let original = std::fs::read(&path)?;
    assert!(
        SongExportRequest::prepare(
            &input,
            &manifest,
            id,
            SongFormat::Audio(AudioFormat::Wav),
            options(),
        )?
        .write_new(&path, &cancel, &AtomicU32::new(0))
        .is_err()
    );
    assert_eq!(std::fs::read(path)?, original);
    Ok(())
}
