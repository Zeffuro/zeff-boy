use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use zeff_emu_common::media::MediaEvent;
use zeff_emu_common::replay::{
    ReplayEvent, ReplayFirmwareManifest, ReplayStartMetadata, ReplayWonderSwanLinkEvent,
};

use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasDeviceIdentity, TasDigest,
    TasExternalIdentity, TasFirmwareIdentity, TasInitialBranch, TasInputFrame, TasInputSpan,
    TasProject, TasProjectIdentity, TasSeekStateCache, TasZapperInput,
};

fn nes_backend() -> EmuBackend {
    let rom = crate::test_support::build_nes_test_rom();
    let emu = zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0)
        .expect("synthetic NES emulator should initialize");
    EmuBackend::from_nes(emu, PathBuf::from("synthetic.nes"))
}

fn fds_backend() -> EmuBackend {
    static BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
        [0xFF; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];
    let side_size = zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE;
    let mut disk = vec![0xA1; side_size];
    disk.resize(side_size * 2, 0xB2);
    load_backend_from_rom_source(
        ActiveSystem::Nes,
        Path::new("synthetic.fds"),
        Path::new("synthetic.fds"),
        Some(disk),
        BackendLoadConfig {
            fds_bios_override: Some(&BIOS),
            ..BackendLoadConfig::default()
        },
    )
    .expect("synthetic FDS backend should initialize")
    .backend
}

fn project_for_backend(
    backend: &EmuBackend,
    frame_count: u64,
    input_spans: Vec<TasInputSpan>,
    events: Vec<ReplayEvent>,
) -> (TasProject, TasProjectIdentity) {
    let start_state = backend
        .encode_state_bytes()
        .expect("synthetic backend should encode state");
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .expect("synthetic backend should expose its media hash"),
    );
    let identity = TasProjectIdentity {
        system: metadata
            .system
            .expect("synthetic backend should expose its system"),
        core_family: metadata
            .core_family
            .expect("synthetic backend should expose its core family"),
        determinism_abi: "synthetic-editor-execution-v1".to_owned(),
        source_media_sha256: effective_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: metadata
            .firmware
            .iter()
            .map(tas_firmware_identity)
            .collect(),
        devices: (1..=5)
            .map(|player| TasDeviceIdentity {
                port: format!("p{player}"),
                device: "materialized-test-input".to_owned(),
                configuration_sha256: TasDigest([player; 32]),
            })
            .collect(),
        sync_config_sha256: TasDigest([0x51; 32]),
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: "synthetic-native-state-v1".to_owned(),
        start_state_sha256: TasDigest::from_bytes(&start_state),
    };
    let project = TasProject::new(
        "synthetic-editor-execution",
        identity.clone(),
        start_state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count,
            input_spans,
            events,
        },
        BTreeMap::new(),
    )
    .expect("synthetic TAS project should validate");
    (project, identity)
}

fn editor(project: TasProject, root: &Path, name: &str) -> TasEditorSession {
    let manual_path = root.join(format!("{name}.ztas"));
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let cache = TasSeekStateCache::open(root.join(format!("{name}-seek-cache"))).unwrap();
    TasEditorSession::new(project, manual_path, autosaves, cache).unwrap()
}

fn engine(backend: EmuBackend, identity: &TasProjectIdentity) -> TasEditorExecutionEngine {
    TasEditorExecutionEngine::new(TasExecutionSession::new(backend, identity.clone()))
}

#[test]
fn exact_start_execution_is_repeatable_and_ignores_prior_backend_input() {
    let root = crate::test_support::test_directory("tas-editor-execution-start").unwrap();
    let backend = nes_backend();
    let input = TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 0x01,
                dpad: 0x10,
            },
            TasControllerInput {
                buttons: 0x02,
                dpad: 0x20,
            },
            TasControllerInput {
                buttons: 0x04,
                dpad: 0x40,
            },
            TasControllerInput {
                buttons: 0x08,
                dpad: 0x80,
            },
            TasControllerInput {
                buttons: 0x10,
                dpad: 0x01,
            },
        ],
        zapper: TasZapperInput {
            enabled: true,
            trigger: true,
            hit: false,
            screen_pos: Some([40, 50]),
        },
        ..TasInputFrame::default()
    };
    let (project, identity) = project_for_backend(
        &backend,
        8,
        vec![TasInputSpan {
            start: 0,
            length: 8,
            input,
        }],
        Vec::new(),
    );

    let mut first_editor = editor(project.clone(), root.path(), "first");
    let mut first = engine(backend, &identity);
    let first_outcome = first.seek(&mut first_editor, 8).unwrap();
    let expected = first.backend().encode_state_bytes().unwrap();
    let expected_framebuffer = first.backend().framebuffer().to_vec();
    assert_eq!(first_outcome.requested_cursor, 8);
    assert!(first_outcome.reached_target());
    assert_eq!(first_outcome.executed_frames, 8);
    assert_eq!(first_outcome.restored_cache_cursor, None);
    assert_eq!(first_outcome.framebuffer.width(), 256);
    assert_eq!(first_outcome.framebuffer.height(), 240);
    assert_eq!(first_outcome.framebuffer.rgba(), expected_framebuffer);
    assert_eq!(
        first_editor.load_seek_state().unwrap(),
        Some(expected.clone())
    );

    let mut dirty_backend = nes_backend();
    dirty_backend.set_input(0xFF, 0xFF);
    dirty_backend.set_input_p2(0xFF, 0xFF);
    dirty_backend.step_frame();
    let mut second_editor = editor(project, root.path(), "second");
    let mut second = engine(dirty_backend, &identity);
    let second_outcome = second.seek(&mut second_editor, 8).unwrap();
    assert_eq!(second_outcome.executed_frames, 8);
    assert_eq!(second.backend().encode_state_bytes().unwrap(), expected);
}

#[test]
fn distant_seek_is_bounded_and_resumes_from_its_exact_intermediate_cache() {
    let root = crate::test_support::test_directory("tas-editor-execution-budget").unwrap();
    let backend = nes_backend();
    let (project, identity) = project_for_backend(&backend, 1_000_001, Vec::new(), Vec::new());
    let mut editor = editor(project, root.path(), "bounded");
    let mut executor = engine(backend, &identity);

    let first = executor
        .seek_with_frame_budget(&mut editor, 900_000, 2)
        .unwrap();
    assert_eq!(first.requested_cursor, 900_000);
    assert_eq!(first.cursor, 2);
    assert_eq!(first.executed_frames, 2);
    assert_eq!(first.remaining_frames(), 899_998);
    assert!(!first.reached_target());
    assert_eq!(editor.cursor(), 2);

    let second = executor
        .seek_with_frame_budget(&mut editor, 900_000, 2)
        .unwrap();
    assert_eq!(second.restored_cache_cursor, Some(2));
    assert_eq!(second.cursor, 4);
    assert_eq!(second.executed_frames, 2);
    assert_eq!(editor.cursor(), 4);
}

#[test]
fn seek_uses_newest_prefix_eligible_cache_and_replays_only_the_changed_suffix() {
    let root = crate::test_support::test_directory("tas-editor-execution-cache").unwrap();
    let backend = nes_backend();
    let (project, identity) = project_for_backend(
        &backend,
        8,
        vec![TasInputSpan {
            start: 0,
            length: 8,
            input: TasInputFrame {
                players: [
                    TasControllerInput {
                        buttons: 1,
                        dpad: 0,
                    },
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                ],
                ..TasInputFrame::default()
            },
        }],
        Vec::new(),
    );
    let mut cached_editor = editor(project, root.path(), "cached");
    let mut executor = engine(backend, &identity);
    executor.seek(&mut cached_editor, 4).unwrap();
    executor.seek(&mut cached_editor, 6).unwrap();
    executor.seek(&mut cached_editor, 8).unwrap();

    cached_editor
        .edit_transaction(|edit| {
            edit.set_input_range(
                "main",
                6,
                1,
                TasInputFrame {
                    players: [
                        TasControllerInput {
                            buttons: 2,
                            dpad: 0,
                        },
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                    ],
                    ..TasInputFrame::default()
                },
            )
        })
        .unwrap();
    let outcome = executor.seek(&mut cached_editor, 8).unwrap();
    assert_eq!(outcome.restored_cache_cursor, Some(6));
    assert_eq!(outcome.executed_frames, 2);

    cached_editor.set_cursor(6).unwrap();
    cached_editor
        .edit_transaction(|edit| {
            edit.set_input_range(
                "main",
                6,
                1,
                TasInputFrame {
                    players: [
                        TasControllerInput {
                            buttons: 3,
                            dpad: 0,
                        },
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                    ],
                    ..TasInputFrame::default()
                },
            )
        })
        .unwrap();
    cached_editor
        .store_seek_state(b"valid cache envelope, invalid emulator state")
        .unwrap();
    let fallback = executor.seek(&mut cached_editor, 8).unwrap();
    assert_eq!(fallback.restored_cache_cursor, Some(4));
    assert_eq!(fallback.executed_frames, 4);
    let actual = executor.backend().encode_state_bytes().unwrap();

    let mut reference_editor = editor(cached_editor.project().clone(), root.path(), "reference");
    let mut reference = engine(nes_backend(), &identity);
    reference.seek(&mut reference_editor, 8).unwrap();
    assert_eq!(reference.backend().encode_state_bytes().unwrap(), actual);
}

#[test]
fn immediate_fds_and_media_events_share_canonical_replay_order() {
    let root = crate::test_support::test_directory("tas-editor-execution-events").unwrap();
    let backend = fds_backend();
    let slot = backend.media_slot_snapshot().unwrap().state.slot;
    let (project, identity) = project_for_backend(
        &backend,
        2,
        Vec::new(),
        vec![
            ReplayEvent::FdsDiskSide { frame: 0, side: 1 },
            ReplayEvent::Media {
                frame: 1,
                sequence: 0,
                event: MediaEvent::SelectSide { slot, side: 0 },
            },
            ReplayEvent::FdsDiskSide { frame: 2, side: 1 },
        ],
    );
    let mut editor = editor(project, root.path(), "events");
    let mut executor = engine(backend, &identity);
    let outcome = executor.seek(&mut editor, 2).unwrap();

    assert_eq!(outcome.events_applied, 3);
    assert_eq!(executor.backend().fds_disk_side(), Some(1));
    let cached_state = editor.load_seek_state().unwrap().unwrap();
    let mut cached_backend = fds_backend();
    cached_backend.load_state_from_bytes(cached_state).unwrap();
    assert_eq!(cached_backend.fds_disk_side(), Some(0));
}

#[test]
fn synchronized_link_domains_are_rejected_without_moving_the_editor_cursor() {
    let root = crate::test_support::test_directory("tas-editor-execution-link").unwrap();
    let backend = nes_backend();
    let (project, identity) = project_for_backend(
        &backend,
        1,
        Vec::new(),
        vec![ReplayEvent::WonderSwanLink {
            frame: 0,
            session_cycle: 1,
            event: ReplayWonderSwanLinkEvent::RemoteByte {
                generation: 1,
                baud_bps: 9_600,
                byte: 0x42,
            },
        }],
    );
    let mut editor = editor(project, root.path(), "link");
    let mut executor = engine(backend, &identity);
    let error = executor.seek(&mut editor, 1).unwrap_err();

    assert!(error.to_string().contains("one emulator endpoint only"));
    assert_eq!(editor.cursor(), 0);
    assert!(editor.load_seek_state().unwrap().is_none());
}

fn tas_firmware_identity(manifest: &ReplayFirmwareManifest) -> TasFirmwareIdentity {
    match manifest {
        ReplayFirmwareManifest::External {
            firmware_id,
            variant,
            sha256,
        } => TasFirmwareIdentity::External {
            firmware_id: firmware_id.clone(),
            variant: variant.clone(),
            sha256: TasDigest(*sha256),
        },
        ReplayFirmwareManifest::Hle {
            firmware_id,
            implementation,
            compatibility_version,
        } => TasFirmwareIdentity::Hle {
            firmware_id: firmware_id.clone(),
            implementation: implementation.clone(),
            compatibility_version: *compatibility_version,
        },
        ReplayFirmwareManifest::BuiltinOpenSource {
            firmware_id,
            implementation,
            compatibility_version,
            sha256,
        } => TasFirmwareIdentity::BuiltinOpenSource {
            firmware_id: firmware_id.clone(),
            implementation: implementation.clone(),
            compatibility_version: *compatibility_version,
            sha256: TasDigest(*sha256),
        },
        ReplayFirmwareManifest::Skipped {
            firmware_id,
            compatibility_version,
        } => TasFirmwareIdentity::Skipped {
            firmware_id: firmware_id.clone(),
            compatibility_version: *compatibility_version,
        },
    }
}
