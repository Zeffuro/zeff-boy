use super::*;
use crate::emu_core_trait::EmulatorCore;
use zeff_emu_common::time::FrameLifecycle;
use zeff_ws_core::hardware::cartridge::compute_footer_checksum;

pub(crate) fn netplay_fixture_rom(color: bool) -> Vec<u8> {
    let mut rom = vec![0xff; 0x10000];
    let mut code = vec![0xfa, 0x31, 0xc0, 0x8e, 0xd8];
    for address in 0x1000_u16..0x1010 {
        code.extend_from_slice(&[0xb0, 0xf0, 0xa2]);
        code.extend_from_slice(&address.to_le_bytes());
    }
    for (port, value) in [
        (0x14, 1),
        (0x80, 0xc0),
        (0x81, 7),
        (0x8f, 0x40),
        (0x90, 1),
        (0x91, 0xf),
        (0xb3, 0xc0),
    ] {
        code.extend_from_slice(&[0xb0, value, 0xe6, port]);
    }
    let loop_offset = code.len();
    for (select, address) in [(0x20, 0x200_u16), (0x10, 0x201), (0x40, 0x202)] {
        code.extend_from_slice(&[0xb0, select, 0xe6, 0xb5, 0xe4, 0xb5, 0xa2]);
        code.extend_from_slice(&address.to_le_bytes());
    }
    code.extend_from_slice(&[
        0xe6, 0x01, 0xe6, 0x88, 0xe6, 0xb1, 0xe4, 0xb1, 0xa2, 3, 2, 0xeb,
    ]);
    code.push((loop_offset as i32 - (code.len() + 1) as i32) as i8 as u8);
    rom[..code.len()].copy_from_slice(&code);
    rom[0xfff0..0xfff5].copy_from_slice(&[0xea, 0, 0, 0, 0xf0]);
    let footer = rom.len() - 10;
    rom[footer + 1] = u8::from(color);
    rom[footer + 4] = 1;
    rom[footer + 5] = 1;
    rom[footer + 6] = 0;
    rom[footer + 7] = 0;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn backend(color: bool) -> WsBackend {
    let rom = netplay_fixture_rom(color);
    let emu = Emulator::new(&rom, 48_000).unwrap();
    WsBackend::new(
        emu,
        format!("synthetic-netplay.{}", if color { "wsc" } else { "ws" }).into(),
    )
}

#[test]
fn endpoint_roles_share_canonical_snapshots_and_restore_exact_local_runtime() {
    for color in [false, true] {
        let mut host = backend(color);
        let mut guest = backend(color);
        let checkpoint_host = host.encode_state_bytes().unwrap();
        let checkpoint_guest = guest.encode_state_bytes().unwrap();
        let initial_runtime = host.netplay_runtime_state_bytes().unwrap();
        let mut host_session = host.begin_netplay_rollback(Endpoint::Zero).unwrap();
        let mut guest_session = guest.begin_netplay_rollback(Endpoint::One).unwrap();
        let host_initial = host_session.capture(&host).unwrap();
        let guest_initial = guest_session.capture(&guest).unwrap();
        assert_eq!(host_initial.checksum(), guest_initial.checksum());
        assert_eq!(
            host_initial.audio_checksum(),
            guest_initial.audio_checksum()
        );
        assert_eq!(
            host_initial.video_checksum(),
            guest_initial.video_checksum()
        );
        assert_eq!(host_initial.native_state().unwrap(), checkpoint_host);
        assert_eq!(guest_initial.native_state().unwrap(), checkpoint_guest);
        assert_eq!(
            host_initial.shared_media_bytes(),
            netplay_fixture_rom(color).len()
        );
        for ports in [[0, 0], [0x0715, 0x02a9], [0x042a, 0x0105]] {
            let host_audio = host_session.advance_frame(&mut host, ports).unwrap();
            let guest_audio = guest_session.advance_frame(&mut guest, ports).unwrap();
            assert!(!host_audio.is_empty() && !guest_audio.is_empty());
            let host_snapshot = host_session.capture(&host).unwrap();
            let guest_snapshot = guest_session.capture(&guest).unwrap();
            assert_eq!(host_snapshot.checksum(), guest_snapshot.checksum());
            assert_eq!(
                host_snapshot.video_checksum(),
                guest_snapshot.video_checksum()
            );
            assert_eq!(
                host_snapshot.audio_checksum(),
                guest_snapshot.audio_checksum()
            );
            assert_eq!(host_snapshot.frame(), host.frame_count());
            assert_eq!(guest_snapshot.frame(), guest.frame_count());
            assert!(host_snapshot.retained_bytes() < 2 * 1024 * 1024);
        }
        host_session
            .restore_after_session(&mut host, &host_initial, &checkpoint_host)
            .unwrap();
        guest_session
            .restore_after_session(&mut guest, &guest_initial, &checkpoint_guest)
            .unwrap();
        assert_eq!(host.encode_state_bytes().unwrap(), checkpoint_host);
        assert_eq!(guest.encode_state_bytes().unwrap(), checkpoint_guest);
        assert_eq!(host.netplay_runtime_state_bytes().unwrap(), initial_runtime);
        assert_eq!(
            guest.netplay_runtime_state_bytes().unwrap(),
            initial_runtime
        );
    }
}

#[test]
fn prediction_correction_restores_both_endpoint_audio_and_inputs() {
    for endpoint in [Endpoint::Zero, Endpoint::One] {
        let mut reference = backend(true);
        let mut predicted = backend(true);
        let mut reference_session = reference.begin_netplay_rollback(endpoint).unwrap();
        let mut predicted_session = predicted.begin_netplay_rollback(endpoint).unwrap();
        let snapshot = predicted_session.capture(&predicted).unwrap();
        let expected = reference_session
            .advance_frame(&mut reference, [0x0715, 0x02a9])
            .unwrap();
        predicted_session
            .advance_frame(&mut predicted, [0, 0])
            .unwrap();
        predicted_session
            .restore(&mut predicted, &snapshot)
            .unwrap();
        let corrected = predicted_session
            .advance_frame(&mut predicted, [0x0715, 0x02a9])
            .unwrap();
        assert_eq!(corrected, expected);
        assert!(corrected.iter().any(|sample| *sample != 0.0));
        let reference_snapshot = reference_session.capture(&reference).unwrap();
        let corrected_snapshot = predicted_session.capture(&predicted).unwrap();
        assert_eq!(corrected_snapshot.checksum(), reference_snapshot.checksum());
        assert_eq!(
            corrected_snapshot.audio_checksum(),
            reference_snapshot.audio_checksum()
        );
        assert_eq!(
            predicted.encode_state_bytes().unwrap(),
            reference.encode_state_bytes().unwrap()
        );
        let row = if endpoint == Endpoint::Zero {
            [0x25, 0x11, 0x4e]
        } else {
            [0x29, 0x1a, 0x48]
        };
        assert_eq!(&predicted.emu.system_ram()[0x200..0x203], &row);
        assert_eq!(
            predicted.emu.system_ram()[0x203],
            if endpoint == Endpoint::Zero {
                0x48
            } else {
                0x4e
            }
        );
    }
}

#[test]
fn foreign_or_mismatching_cleanup_checkpoints_fail_closed() {
    let mut backend = backend(false);
    let mut session = backend.begin_netplay_rollback(Endpoint::Zero).unwrap();
    let original = session.capture(&backend).unwrap();
    let foreign_session = backend.begin_netplay_rollback(Endpoint::Zero).unwrap();
    let foreign = foreign_session.capture(&backend).unwrap();
    session.advance_frame(&mut backend, [1, 2]).unwrap();
    let state = backend.encode_state_bytes().unwrap();
    assert!(session.restore(&mut backend, &foreign).is_err());
    assert!(
        session
            .restore_after_session(&mut backend, &original, &[0])
            .is_err()
    );
    assert_eq!(state, backend.encode_state_bytes().unwrap());
    backend.set_host_persistence_enabled(false);
    assert!(!backend.host_persistence_enabled());
    assert!(backend.flush_battery_sram().unwrap().is_none());
    assert!(session.advance_frame(&mut backend, [0x8000, 0]).is_err());
}

#[test]
fn provenance_rejects_runtime_configuration_and_tracks_loaded_state() {
    let mut backend = backend(false);
    let rom = netplay_fixture_rom(false);
    let source = zeff_firmware::sha256_bytes(&rom);
    backend
        .capture_netplay_load_provenance(source, rom.len(), true, true, true)
        .unwrap();
    let provenance = backend.netplay_load_provenance().unwrap();
    assert!(provenance.authenticated_source && provenance.unmodified && provenance.neutral_input);
    assert_eq!(provenance.source, source);
    assert_eq!(
        provenance.state,
        zeff_firmware::sha256_bytes(&backend.encode_state_bytes().unwrap())
    );
    assert_eq!(
        provenance.pair_checksum,
        backend.netplay_initial_pair_checksum().unwrap()
    );
    backend.set_apu_channel_mutes(&[true]);
    assert!(backend.validate_netplay_boundary().is_err());
    backend
        .capture_netplay_load_provenance(source, rom.len(), true, true, true)
        .unwrap();
    assert!(backend.netplay_load_provenance().is_none());
}

#[test]
fn audio_hash_preserves_endpoint_order_and_sample_lengths() {
    assert_ne!(hash_audio([&[1.0], &[]]), hash_audio([&[], &[1.0]]));
    assert_ne!(hash_audio([&[1.0], &[2.0]]), hash_audio([&[1.0, 2.0], &[]]));
    assert_ne!(hash_audio([&[0.0], &[]]), hash_audio([&[-0.0], &[]]));
}

#[test]
fn rtc_persistence_is_canonical_and_restored_with_owned_runtime() {
    let mut rom = netplay_fixture_rom(true);
    let footer = rom.len() - 10;
    rom[footer + 7] = 1;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..].copy_from_slice(&checksum.to_le_bytes());
    let mut local = WsBackend::new(Emulator::new(&rom, 48_000).unwrap(), "rtc.wsc".into());
    let before = local.netplay_persistent_state_bytes();
    let checkpoint = local.encode_state_bytes().unwrap();
    let mut session = local.begin_netplay_rollback(Endpoint::One).unwrap();
    let initial = session.capture(&local).unwrap();
    assert_eq!(
        initial.persistent_checksum(),
        local.netplay_initial_persistent_checksum()
    );
    session.advance_frame(&mut local, [0x0715, 0x02a9]).unwrap();
    let advanced = session.capture(&local).unwrap();
    assert_ne!(
        advanced.persistent_checksum(),
        initial.persistent_checksum()
    );
    session
        .restore_after_session(&mut local, &initial, &checkpoint)
        .unwrap();
    assert_eq!(before, local.netplay_persistent_state_bytes());
    assert_eq!(
        session.capture(&local).unwrap().persistent_checksum(),
        initial.persistent_checksum()
    );
}

#[test]
fn external_mutations_reject_capture_and_cleanup_without_losing_save_fence() {
    let mut backend = backend(false);
    let mut session = backend.begin_netplay_rollback(Endpoint::Zero).unwrap();
    let original = session.capture(&backend).unwrap();
    let checkpoint = original.native_state().unwrap();
    backend.set_host_persistence_enabled(false);
    session.advance_frame(&mut backend, [1, 2]).unwrap();
    backend.emu.cpu_write8(0x300, 0x12);
    assert!(session.capture(&backend).is_err());
    assert!(
        session
            .restore_after_session(&mut backend, &original, &checkpoint)
            .is_err()
    );
    assert!(!backend.host_persistence_enabled());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn loader_authenticates_direct_and_selected_zip_ws_and_wsc_bytes() {
    use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
    use std::io::Write as _;

    let directory = crate::test_support::test_directory("ws-netplay-provenance").unwrap();
    for color in [false, true] {
        let extension = if color { "wsc" } else { "ws" };
        let bytes = netplay_fixture_rom(color);
        let direct = directory.path().join(format!("direct.{extension}"));
        std::fs::write(&direct, &bytes).unwrap();
        let load = |source: &std::path::Path, rom: &std::path::Path, data, sample_rate| {
            load_backend_from_rom_source(
                ActiveSystem::WonderSwan,
                source,
                rom,
                data,
                BackendLoadConfig {
                    sample_rate: Some(sample_rate),
                    ws_load_battery_sram: false,
                    ..Default::default()
                },
            )
            .unwrap()
            .backend
        };
        let backend = load(&direct, &direct, None, 48_000);
        let expected = backend.ws().unwrap().netplay_load_provenance().unwrap();
        assert!(expected.authenticated_source);
        assert_eq!(expected.source, zeff_firmware::sha256_bytes(&bytes));
        for method in [
            zip::CompressionMethod::Stored,
            zip::CompressionMethod::Deflated,
        ] {
            let archive_path = directory.path().join(format!("container-{extension}.zip"));
            let member = format!("nested/game.{extension}");
            let mut archive = zip::ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
            archive
                .start_file(
                    &member,
                    zip::write::SimpleFileOptions::default().compression_method(method),
                )
                .unwrap();
            archive.write_all(&bytes).unwrap();
            archive.finish().unwrap();
            let selected = archive_path.join(&member);
            let backend = load(&archive_path, &selected, Some(bytes.clone()), 48_000);
            let actual = backend.ws().unwrap().netplay_load_provenance().unwrap();
            assert!(actual.authenticated_source);
            assert_eq!(actual.source, expected.source);
            assert_eq!(actual.pair_checksum, expected.pair_checksum);
            assert_eq!(actual.persistent, expected.persistent);
            let stale = load(
                &archive_path,
                &archive_path.join(format!("missing.{extension}")),
                Some(bytes.clone()),
                48_000,
            );
            assert!(
                !stale
                    .ws()
                    .unwrap()
                    .netplay_load_provenance()
                    .unwrap()
                    .authenticated_source
            );
        }
        let unauthenticated = load(&direct, &direct, Some(bytes.clone()), 48_000);
        assert!(
            !unauthenticated
                .ws()
                .unwrap()
                .netplay_load_provenance()
                .unwrap()
                .authenticated_source
        );
        let ordinary = load(&direct, &direct, None, 44_100);
        assert!(ordinary.ws().unwrap().netplay_load_provenance().is_none());
    }
}
