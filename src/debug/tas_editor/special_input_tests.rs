use std::collections::BTreeMap;

use zeff_emu_common::replay::POCKET_CAMERA_FRAME_BYTES;
use zeff_emu_common::replay::ReplayStartMetadata;

use super::special_input_editor::{
    GAME_BOY_MBC7_DEVICE, GAME_BOY_POCKET_CAMERA_DEVICE, GBA_TILT_DEVICE, NES_ZAPPER_DEVICE,
    TasSpecialInputAction, TasSpecialInputMutation, camera_asset_is_authorable,
    special_input_capabilities,
};
use super::*;
use crate::emu_backend::loader::direct_nes_tas_identity;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::tas_project::verification::TasExecutionSession;
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasInitialBranch,
    TasInputFrame, TasInputSpan, TasProject, TasProjectIdentity, TasZapperInput,
};

fn synthetic_identity(
    system: &str,
    device_names: &[&str],
    start_state: &[u8],
) -> TasProjectIdentity {
    TasProjectIdentity {
        system: system.to_owned(),
        core_family: "special-input-test-core".to_owned(),
        determinism_abi: "special-input-test-v1".to_owned(),
        source_media_sha256: TasDigest([1; 32]),
        effective_media_sha256: TasDigest([2; 32]),
        patches: Vec::new(),
        firmware: Vec::new(),
        devices: device_names
            .iter()
            .enumerate()
            .map(|(index, device)| TasDeviceIdentity {
                port: format!("special-{index}"),
                device: (*device).to_owned(),
                configuration_sha256: TasDigest([index as u8 + 3; 32]),
            })
            .collect(),
        sync_config_sha256: TasDigest([8; 32]),
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: "special-input-test-state-v1".to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    }
}

pub(super) fn synthetic_state(
    system: &str,
    devices: &[&str],
    initial_input: TasInputFrame,
    assets: BTreeMap<TasDigest, Vec<u8>>,
) -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    let root = crate::test_support::test_directory("tas-editor-special-input").unwrap();
    let start_state = vec![0x5A; 16];
    let input_spans = (initial_input != TasInputFrame::default())
        .then_some(TasInputSpan {
            start: 0,
            length: 1,
            input: initial_input,
        })
        .into_iter()
        .collect();
    let project = TasProject::new(
        "special-input-project",
        synthetic_identity(system, devices, &start_state),
        start_state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 3,
            input_spans,
            events: Vec::new(),
        },
        assets,
    )
    .unwrap();
    let manual = root.path().join("movie.ztas");
    project.save_atomic(&manual).unwrap();
    let mut state = TasEditorWindowState::with_seek_cache_root(root.path().join("seek-cache"));
    state.reduce(TasEditorAction::OpenProject(manual)).unwrap();
    (root, state)
}

fn executable_zapper_state() -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    let root = crate::test_support::test_directory("tas-editor-special-zapper-execution").unwrap();
    let rom_path = root.path().join("game.nes");
    let rom = crate::test_support::build_nes_test_rom();
    std::fs::write(&rom_path, &rom).unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        Some(rom.clone()),
        BackendLoadConfig {
            sample_rate: None,
            apply_mods: false,
            initial_input: None,
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    let start_state = backend.encode_state_bytes().unwrap();
    let mut identity = direct_nes_tas_identity(&backend, &rom, &start_state).unwrap();
    identity.devices.push(TasDeviceIdentity {
        port: "expansion".to_owned(),
        device: NES_ZAPPER_DEVICE.to_owned(),
        configuration_sha256: TasDigest::from_bytes(b"synthetic-zapper-config-v1"),
    });
    let project = TasProject::new(
        "special-zapper-execution",
        identity.clone(),
        start_state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 3,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )
    .unwrap();
    let manual = root.path().join("movie.ztas");
    project.save_atomic(&manual).unwrap();
    let mut state = TasEditorWindowState::with_seek_cache_root(root.path().join("seek-cache"));
    state.reduce(TasEditorAction::OpenProject(manual)).unwrap();
    state.execution_engine = Some(TasEditorExecutionEngine::new(TasExecutionSession::new(
        backend, identity,
    )));
    (root, state)
}

fn reduce_special(
    state: &mut TasEditorWindowState,
    cursor: u64,
    mutation: TasSpecialInputMutation,
) -> anyhow::Result<Option<String>> {
    let session = state.session.as_ref().unwrap();
    let action = TasSpecialInputAction::new(
        session.project_content_sha256(),
        session.selected_branch_id().to_owned(),
        cursor,
        mutation,
    );
    state.reduce(TasEditorAction::SpecialInput(action))
}

#[test]
fn capabilities_require_exact_system_and_device_identity() {
    let start_state = [0xA5; 4];
    let standard_nes = synthetic_identity("nes", &["nes-standard-controller"], &start_state);
    assert_eq!(
        special_input_capabilities(&standard_nes),
        Default::default()
    );

    let zapper = synthetic_identity("nes", &[NES_ZAPPER_DEVICE], &start_state);
    assert!(special_input_capabilities(&zapper).nes_zapper);
    let wrong_system = synthetic_identity("pce", &[NES_ZAPPER_DEVICE], &start_state);
    assert_eq!(
        special_input_capabilities(&wrong_system),
        Default::default()
    );

    let game_boy = synthetic_identity(
        "gb",
        &[GAME_BOY_MBC7_DEVICE, GAME_BOY_POCKET_CAMERA_DEVICE],
        &start_state,
    );
    let capabilities = special_input_capabilities(&game_boy);
    assert!(capabilities.mbc7_tilt);
    assert!(capabilities.pocket_camera);
    assert!(!capabilities.nes_zapper);

    let gba = synthetic_identity("gba", &[GBA_TILT_DEVICE], &start_state);
    let capabilities = special_input_capabilities(&gba);
    assert!(capabilities.gba_tilt);
    assert!(!capabilities.mbc7_tilt);

    let wrong_gba_system = synthetic_identity("gb", &[GBA_TILT_DEVICE], &start_state);
    assert!(!special_input_capabilities(&wrong_gba_system).gba_tilt);
}

#[test]
fn gba_tilt_is_authorable_with_exact_raw_bits() {
    let (_root, mut state) = synthetic_state(
        "gba",
        &[GBA_TILT_DEVICE],
        TasInputFrame::default(),
        BTreeMap::new(),
    );
    let (x_bits, y_bits) = (0x7FC0_0123, 0xBF00_0000);
    reduce_special(
        &mut state,
        1,
        TasSpecialInputMutation::RecordedTilt { x_bits, y_bits },
    )
    .unwrap();
    let branch = state.session.as_ref().unwrap().selected_branch();
    assert_eq!(
        (
            branch.input_at(1).tilt_x_bits,
            branch.input_at(1).tilt_y_bits
        ),
        (x_bits, y_bits)
    );
    assert_eq!(branch.input_at(0), TasInputFrame::default());
    assert_eq!(branch.input_at(2), TasInputFrame::default());
}

#[test]
fn applicable_channels_replace_only_the_selected_frame_exactly() {
    let (_root, mut zapper_state) = synthetic_state(
        "nes",
        &[NES_ZAPPER_DEVICE],
        TasInputFrame::default(),
        BTreeMap::new(),
    );
    let zapper = TasZapperInput {
        enabled: true,
        trigger: true,
        hit: false,
        screen_pos: Some([255, 239]),
    };
    reduce_special(
        &mut zapper_state,
        1,
        TasSpecialInputMutation::NesZapper(zapper),
    )
    .unwrap();
    let branch = zapper_state.session.as_ref().unwrap().selected_branch();
    assert_eq!(branch.input_at(1).zapper, zapper);
    assert_eq!(branch.input_at(0), TasInputFrame::default());
    assert_eq!(branch.input_at(2), TasInputFrame::default());

    let mut initial_tilt_frame = TasInputFrame::default();
    initial_tilt_frame.players[2].buttons = 0xA5;
    initial_tilt_frame.players[2].dpad = 0x5A;
    let (_root, mut tilt_state) = synthetic_state(
        "gb",
        &[GAME_BOY_MBC7_DEVICE],
        initial_tilt_frame,
        BTreeMap::new(),
    );
    let (x_bits, y_bits) = (0x7FC0_0123, 0x8000_0000);
    reduce_special(
        &mut tilt_state,
        0,
        TasSpecialInputMutation::RecordedTilt { x_bits, y_bits },
    )
    .unwrap();
    let input = tilt_state
        .session
        .as_ref()
        .unwrap()
        .selected_branch()
        .input_at(0);
    assert_eq!((input.tilt_x_bits, input.tilt_y_bits), (x_bits, y_bits));
    assert_eq!(input.players, initial_tilt_frame.players);
    assert_eq!(input.zapper, initial_tilt_frame.zapper);
    assert_eq!(input.camera, initial_tilt_frame.camera);
}

#[test]
fn camera_selection_uses_only_an_existing_content_addressed_asset() {
    let bytes = vec![0x10; POCKET_CAMERA_FRAME_BYTES];
    let digest = TasDigest::from_bytes(&bytes);
    let wrong_bytes = vec![0x20; POCKET_CAMERA_FRAME_BYTES - 1];
    let wrong_digest = TasDigest::from_bytes(&wrong_bytes);
    assert!(camera_asset_is_authorable(&bytes));
    assert!(!camera_asset_is_authorable(&wrong_bytes));
    let (_root, mut state) = synthetic_state(
        "gb",
        &[GAME_BOY_POCKET_CAMERA_DEVICE],
        TasInputFrame::default(),
        BTreeMap::from([(digest, bytes), (wrong_digest, wrong_bytes)]),
    );
    let wrong_size_error = reduce_special(
        &mut state,
        0,
        TasSpecialInputMutation::PocketCamera(TasCameraInput::Blob(wrong_digest)),
    )
    .unwrap_err();
    assert!(wrong_size_error.to_string().contains("128x112"));
    reduce_special(
        &mut state,
        0,
        TasSpecialInputMutation::PocketCamera(TasCameraInput::Blob(digest)),
    )
    .unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(0)
            .camera,
        TasCameraInput::Blob(digest)
    );

    let missing = TasDigest([0xEE; 32]);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    let error = reduce_special(
        &mut state,
        1,
        TasSpecialInputMutation::PocketCamera(TasCameraInput::Blob(missing)),
    )
    .unwrap_err();
    assert!(error.to_string().contains("missing camera asset"));
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

#[test]
fn legacy_wrong_sized_camera_reference_is_preserved_until_explicitly_cleared() {
    let bytes = vec![0x44; 7];
    let digest = TasDigest::from_bytes(&bytes);
    let initial_input = TasInputFrame {
        camera: TasCameraInput::Blob(digest),
        ..TasInputFrame::default()
    };
    let (_root, mut state) = synthetic_state(
        "gb",
        &[GAME_BOY_POCKET_CAMERA_DEVICE],
        initial_input,
        BTreeMap::from([(digest, bytes)]),
    );

    reduce_special(
        &mut state,
        0,
        TasSpecialInputMutation::PocketCamera(TasCameraInput::Blob(digest)),
    )
    .unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(0)
            .camera,
        TasCameraInput::Blob(digest)
    );
    reduce_special(
        &mut state,
        0,
        TasSpecialInputMutation::PocketCamera(TasCameraInput::None),
    )
    .unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(0)
            .camera,
        TasCameraInput::None
    );
}

#[test]
fn successful_movie_edit_clears_preview_and_preserves_history_rerecord_autosave() {
    let (_root, mut state) = executable_zapper_state();
    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    assert!(state.execution_preview.exact_frame().is_some());

    reduce_special(
        &mut state,
        1,
        TasSpecialInputMutation::NesZapper(TasZapperInput {
            enabled: true,
            trigger: true,
            hit: true,
            screen_pos: Some([12, 34]),
        }),
    )
    .unwrap();
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project().edit_generation(), 1);
    assert_eq!(session.project().rerecord_count(), 1);
    assert!(session.can_undo());
    assert!(state.execution_preview.exact_frame().is_none());
    assert!(state.execution_engine.is_some());

    state.reduce(TasEditorAction::Autosave).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().last_autosaved_generation(),
        Some(1)
    );
    state.reduce(TasEditorAction::Undo).unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(1),
        TasInputFrame::default()
    );
    state.reduce(TasEditorAction::Redo).unwrap();
    assert!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(1)
            .zapper
            .trigger
    );
    assert_eq!(
        state.session.as_ref().unwrap().project().rerecord_count(),
        1
    );
    assert_eq!(
        state.session.as_ref().unwrap().last_autosaved_generation(),
        Some(1)
    );
}

fn reject_zapper_scope(project: &TasProject, branch_id: &str) -> anyhow::Result<()> {
    let branch = project
        .branch(branch_id)
        .ok_or_else(|| anyhow::anyhow!("unknown test branch"))?;
    if branch
        .input_spans()
        .iter()
        .any(|span| span.input.zapper != TasZapperInput::default())
    {
        anyhow::bail!("synthetic execution profile rejects Zapper input");
    }
    Ok(())
}

#[test]
fn changed_input_detaches_an_execution_profile_that_no_longer_accepts_the_project() {
    let (_root, mut state) = executable_zapper_state();
    let old_engine = state.execution_engine.take().unwrap();
    let identity = state.session.as_ref().unwrap().project().identity().clone();
    let engine = TasEditorExecutionEngine::attach(
        state.session.as_ref().unwrap().project(),
        TasExecutionSession::new(old_engine.into_backend(), identity),
        reject_zapper_scope,
    )
    .unwrap();
    state.execution_engine = Some(engine);

    let message = reduce_special(
        &mut state,
        1,
        TasSpecialInputMutation::NesZapper(TasZapperInput {
            enabled: true,
            ..TasZapperInput::default()
        }),
    )
    .unwrap()
    .unwrap();

    assert!(state.execution_engine.is_none());
    assert!(message.contains("private execution detached"));
    assert!(message.contains("rejects Zapper input"));
}

#[test]
fn failed_action_rolls_back_and_preserves_exact_preview() {
    let (_root, mut state) = executable_zapper_state();
    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    let before = state.session.as_ref().unwrap().project().encode().unwrap();

    let error = reduce_special(
        &mut state,
        3,
        TasSpecialInputMutation::NesZapper(TasZapperInput {
            enabled: true,
            ..TasZapperInput::default()
        }),
    )
    .unwrap_err();

    assert!(error.to_string().contains("end cursor"));
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.project().edit_generation(), 0);
    assert!(!session.can_undo());
    assert!(state.execution_preview.exact_frame().is_some());
}

#[test]
fn unsupported_legacy_value_can_only_be_cleared() {
    let initial_input = TasInputFrame {
        zapper: TasZapperInput {
            enabled: true,
            trigger: true,
            ..TasZapperInput::default()
        },
        ..TasInputFrame::default()
    };
    let (_root, mut state) = synthetic_state(
        "nes",
        &["nes-standard-controller"],
        initial_input,
        BTreeMap::new(),
    );
    let error = reduce_special(
        &mut state,
        0,
        TasSpecialInputMutation::NesZapper(TasZapperInput {
            enabled: true,
            trigger: false,
            ..TasZapperInput::default()
        }),
    )
    .unwrap_err();
    assert!(error.to_string().contains("does not declare"));

    reduce_special(
        &mut state,
        0,
        TasSpecialInputMutation::NesZapper(TasZapperInput::default()),
    )
    .unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(0),
        TasInputFrame::default()
    );
}

#[test]
fn queued_action_fails_closed_after_an_exact_project_revision_change() {
    let (_root, mut state) = synthetic_state(
        "gb",
        &[GAME_BOY_MBC7_DEVICE],
        TasInputFrame::default(),
        BTreeMap::new(),
    );
    let session = state.session.as_ref().unwrap();
    let stale = TasSpecialInputAction::new(
        session.project_content_sha256(),
        "main".to_owned(),
        1,
        TasSpecialInputMutation::RecordedTilt {
            x_bits: 0x3F80_0000,
            y_bits: 0,
        },
    );
    reduce_special(
        &mut state,
        0,
        TasSpecialInputMutation::RecordedTilt {
            x_bits: 0,
            y_bits: 0x4000_0000,
        },
    )
    .unwrap();
    let after_first_edit = state.session.as_ref().unwrap().project().encode().unwrap();

    let error = state
        .reduce(TasEditorAction::SpecialInput(stale))
        .unwrap_err();

    assert!(error.to_string().contains("project changed"));
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        after_first_edit
    );
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(1),
        TasInputFrame::default()
    );
}
