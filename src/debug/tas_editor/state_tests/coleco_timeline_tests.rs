use std::collections::BTreeMap;

use super::super::{ColecoControl, TasEditorAction, TasEditorWindowState};
use crate::tas_project::{
    TasColecoKeypadKey, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasInitialBranch,
    TasProject, TasProjectIdentity,
};
use zeff_emu_common::replay::ReplayStartMetadata;

fn coleco_project(frame_count: u64) -> TasProject {
    let start_state = vec![0xC0; 16];
    TasProject::new(
        "coleco-ui-project",
        TasProjectIdentity {
            system: "coleco".to_owned(),
            core_family: "coleco".to_owned(),
            determinism_abi: "coleco-test-sync-v1".to_owned(),
            source_media_sha256: TasDigest([1; 32]),
            effective_media_sha256: TasDigest([2; 32]),
            patches: Vec::new(),
            firmware: Vec::new(),
            devices: (1..=2)
                .map(|player| TasDeviceIdentity {
                    port: format!("p{player}"),
                    device: "coleco-standard-controller-keypad".to_owned(),
                    configuration_sha256: TasDigest([player as u8; 32]),
                })
                .collect(),
            sync_config_sha256: TasDigest([3; 32]),
            persistent_state: TasExternalIdentity::Absent,
            rtc_state: TasExternalIdentity::Absent,
            sensor_state: TasExternalIdentity::Absent,
            cheats: TasExternalIdentity::Absent,
            state_format_compatibility_id: "coleco-test-state-v1".to_owned(),
            start_state_sha256: TasDigest::from_bytes(&start_state),
        },
        start_state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )
    .unwrap()
}

fn state_with_coleco_project(
    frame_count: u64,
) -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    let root = crate::test_support::test_directory("tas-editor-coleco-ui").unwrap();
    let manual = root.path().join("movie.ztas");
    coleco_project(frame_count).save_atomic(&manual).unwrap();
    let mut state = TasEditorWindowState::with_seek_cache_root(root.path().join("seek-cache"));
    state.reduce(TasEditorAction::OpenProject(manual)).unwrap();
    (root, state)
}

#[test]
fn coleco_timeline_edits_keep_both_controller_and_keypad_domains() {
    let (_root, mut state) = state_with_coleco_project(2);
    state
        .reduce(TasEditorAction::ToggleColecoControl {
            cursor: 1,
            player: 0,
            control: ColecoControl::LeftButton,
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SetColecoKeypad {
            cursor: 1,
            player: 0,
            key: TasColecoKeypadKey::Star,
        })
        .unwrap();
    state
        .reduce(TasEditorAction::ToggleColecoControl {
            cursor: 1,
            player: 1,
            control: ColecoControl::Right,
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SetColecoKeypad {
            cursor: 1,
            player: 1,
            key: TasColecoKeypadKey::Nine,
        })
        .unwrap();

    let input = state
        .session
        .as_ref()
        .unwrap()
        .selected_branch()
        .input_at(1);
    assert!(input.coleco[0].left_button);
    assert_eq!(input.coleco[0].keypad, TasColecoKeypadKey::Star);
    assert!(input.coleco[1].right);
    assert_eq!(input.coleco[1].keypad, TasColecoKeypadKey::Nine);
    assert_eq!(
        input.players,
        [crate::tas_project::TasControllerInput::default(); 5]
    );
}
