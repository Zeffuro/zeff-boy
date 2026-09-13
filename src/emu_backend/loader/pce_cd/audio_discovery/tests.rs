use super::*;
use std::io::{Cursor, Write};
use std::sync::atomic::AtomicBool;

use crate::audio_discovery::ScanStatus;
use crate::audio_discovery::catalog::SongId;
use crate::audio_discovery::formats::{AudioFormat, SongFormat};
use crate::emu_backend::loader::{PreparedNativeArchiveBackend, prepare_native_archive_backend};
use crate::emu_backend::{BackendLoadConfig, pce_cd_archive::PceCdPackageProgress};

fn archive(path: &Path, entries: &[(&str, Vec<u8>)]) -> anyhow::Result<()> {
    if path.extension().is_some_and(|ext| ext == "7z") {
        let mut writer = sevenz_rust2::ArchiveWriter::create(path)?;
        for (name, bytes) in entries {
            writer.push_archive_entry(
                sevenz_rust2::ArchiveEntry::new_file(name),
                Some(Cursor::new(bytes.clone())),
            )?;
        }
        writer.finish()?;
    } else {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path)?);
        for (name, bytes) in entries {
            writer.start_file(*name, zip::write::SimpleFileOptions::default())?;
            writer.write_all(bytes)?;
        }
        writer.finish()?;
    }
    Ok(())
}

#[test]
fn audio_discovery_receives_normal_archive_disc_without_tas_provenance() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let audio: Vec<u8> = (0..3 * 588)
        .flat_map(|frame| [frame as i16, -(frame as i16)])
        .flat_map(i16::to_le_bytes)
        .collect();
    let entries = [
        ("set/data.bin", vec![0; 2048]),
        ("set/audio.bin", audio.clone()),
        (
            "set/disc.cue",
            b"FILE \"data.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"audio.bin\" BINARY\nTRACK 02 AUDIO\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n".to_vec(),
        ),
    ];
    let config = BackendLoadConfig {
        pce_cd_system_card_override: Some(Box::leak(vec![0; 262_144].into_boxed_slice())),
        pce_cd_system_card_sha256_override: Some(zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256),
        pce_console_wiring: Some(zeff_pce_core::hardware::PceConsoleWiring::TurboGrafx16),
        pce_load_battery_bram: false,
        apply_mods: false,
        ..Default::default()
    };
    for extension in ["7z", "zip"] {
        let path = directory.path().join(format!("music.{extension}"));
        archive(&path, &entries)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(PceCdPackageProgress::default());
        let PreparedNativeArchiveBackend::Ready { loaded, .. } =
            prepare_native_archive_backend(&path, None, None, &config, &cancel, &progress)?
        else {
            panic!("a single CUE requires no member selection");
        };
        let input = loaded.backend.audio_discovery_input().unwrap();
        assert!(Arc::ptr_eq(
            &input,
            &loaded.backend.audio_discovery_input().unwrap()
        ));
        let mut worker = crate::emu_thread::EmuThread::spawn(loaded.backend, false);
        assert!(Arc::ptr_eq(
            &input,
            &worker.audio_discovery_input().unwrap()
        ));
        let mut state = crate::debug::AudioDiscoveryState::default();
        state.bind_source(Some(Arc::clone(&input)));
        state.session.start();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.session.is_busy() {
            assert!(std::time::Instant::now() < deadline);
            state.session.poll();
            std::thread::yield_now();
        }
        let manifest = state.session.manifest.as_ref().unwrap();
        assert_eq!(manifest.scan.status, ScanStatus::Complete);
        assert_eq!(manifest.scan.song_count(), 1);
        let track = manifest.scan.cdda_tracks[0];
        assert_eq!(track.number, 2);
        assert_eq!(track.pcm_frames, 2 * 588);
        assert!(
            manifest
                .scan
                .song(SongId::Cdda(0))
                .unwrap()
                .supports(SongFormat::Audio(AudioFormat::Wav))
        );
        let output = directory.path().join(format!("{extension}-track.wav"));
        crate::audio_discovery::cdda::write_new(
            input.cdda.as_ref().unwrap(),
            2,
            AudioFormat::Wav,
            &output,
            &cancel,
        )?;
        let mut reader = hound::WavReader::open(output)?;
        let pcm: Vec<u8> = reader
            .samples::<i16>()
            .map(Result::unwrap)
            .flat_map(i16::to_le_bytes)
            .collect();
        assert_eq!(pcm, audio[2352..]);
        state.bind_source(None);
        assert!(state.session.manifest.is_none());
        worker.shutdown();
    }
    Ok(())
}
