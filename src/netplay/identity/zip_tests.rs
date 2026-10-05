use super::*;
use crate::emu_backend::{BackendLoadConfig, load_backend_from_rom_source};
use std::io::Write as _;
use std::net::{TcpListener, TcpStream};
use std::path::Path;

fn fixture(system: ActiveSystem) -> (&'static str, Vec<u8>) {
    match system {
        ActiveSystem::Nes => ("nes", crate::test_support::build_nes_test_rom()),
        ActiveSystem::Pce => ("pce", crate::emu_backend::pce::netplay_fixture_hucard()),
        ActiveSystem::MasterSystem | ActiveSystem::Sg1000 => {
            let mut rom = vec![0; 32768];
            rom[..3].copy_from_slice(&[0xc3, 0x00, 0x00]);
            (
                if system == ActiveSystem::MasterSystem {
                    "sms"
                } else {
                    "sg"
                },
                rom,
            )
        }
        _ => unreachable!(),
    }
}

fn archive(path: &Path, member: &str, bytes: &[u8], method: zip::CompressionMethod) {
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    writer
        .start_file(
            member,
            zip::write::SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
    writer.write_all(bytes).unwrap();
    writer.finish().unwrap();
}

fn load(system: ActiveSystem, source: &Path, rom: &Path, bytes: Option<Vec<u8>>) -> EmuBackend {
    load_backend_from_rom_source(
        system,
        source,
        rom,
        bytes,
        BackendLoadConfig {
            sample_rate: Some(48_000),
            nes_load_battery_sram: false,
            sega8_load_battery_sram: false,
            pce_load_battery_bram: false,
            pce_netplay: system == ActiveSystem::Pce,
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

fn admit(local: Identity, remote: Identity, compatible: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    let other = std::thread::spawn(move || {
        zeff_netplay::wire::admit(peer, zeff_netplay::lockstep::Player::Two, &remote, &[5; 32])
    });
    assert_eq!(
        zeff_netplay::wire::admit(
            stream,
            zeff_netplay::lockstep::Player::One,
            &local,
            &[5; 32]
        )
        .is_ok(),
        compatible
    );
    assert_eq!(other.join().unwrap().is_ok(), compatible);
}

#[test]
fn selected_zip_and_direct_roms_admit_equally_for_every_supported_console() {
    for system in [
        ActiveSystem::Nes,
        ActiveSystem::MasterSystem,
        ActiveSystem::Sg1000,
        ActiveSystem::Pce,
    ] {
        let directory = crate::test_support::test_directory("netplay-zip-identity").unwrap();
        let (extension, bytes) = fixture(system);
        let direct = directory.path().join(format!("direct.{extension}"));
        std::fs::write(&direct, &bytes).unwrap();
        let direct_backend = load(system, &direct, &direct, None);
        let expected = identity(&direct_backend, [7; 32]).unwrap();
        let mut previous = expected.clone();
        for (index, method) in [
            zip::CompressionMethod::Stored,
            zip::CompressionMethod::Deflated,
        ]
        .into_iter()
        .enumerate()
        {
            let zipped = directory.path().join(format!("container-{index}.zip"));
            let member = format!("folder-{index}/renamed.{extension}");
            archive(&zipped, &member, &bytes, method);
            let backend = load(system, &zipped, &zipped.join(&member), Some(bytes.clone()));
            let actual = identity(&backend, [7; 32]).unwrap();
            assert_eq!(actual, expected, "{system:?}");
            admit(previous, actual.clone(), true);
            previous = actual;
            if system == ActiveSystem::Nes {
                let provenance = backend.nes_tas_load_provenance().unwrap().load;
                assert!(!provenance.direct_nes_file);
                assert_ne!(provenance.raw_source_media_sha256, expected.source);
                assert_ne!(
                    provenance.sync_config_sha256,
                    provenance.netplay_media.unwrap().policy
                );
            }
            let mut replaced = bytes.clone();
            *replaced.last_mut().unwrap() ^= 1;
            archive(&zipped, &member, &replaced, method);
            assert_eq!(identity(&backend, [7; 32]).unwrap(), expected);
            let stale = load(system, &zipped, &zipped.join(&member), Some(bytes.clone()));
            assert!(identity(&stale, [7; 32]).is_err());
            let changed = load(system, &zipped, &zipped.join(&member), Some(replaced));
            admit(
                expected.clone(),
                identity(&changed, [7; 32]).unwrap(),
                false,
            );
        }
    }
}

#[test]
fn selected_zip_extraction_rejects_unsafe_missing_and_oversized_members() {
    let directory = crate::test_support::test_directory("netplay-zip-boundaries").unwrap();
    let path = directory.path().join("container.zip");
    for member in ["../game.nes", "game.nes"] {
        archive(&path, member, &[0; 5], zip::CompressionMethod::Stored);
        assert!(
            crate::rom_archive::extract_bounded_zip_member(
                &path,
                Some(&path.join(member)),
                "nes",
                4096,
                4
            )
            .is_err()
        );
    }
    archive(&path, "game.nes", &[0; 4], zip::CompressionMethod::Stored);
    assert!(
        crate::rom_archive::extract_bounded_zip_member(
            &path,
            Some(&path.join("missing.nes")),
            "nes",
            4096,
            4
        )
        .is_err()
    );
    assert!(
        crate::rom_archive::extract_bounded_zip_member(
            &path,
            Some(&path.join("game.nes")),
            "nes",
            1,
            4
        )
        .is_err()
    );
}
