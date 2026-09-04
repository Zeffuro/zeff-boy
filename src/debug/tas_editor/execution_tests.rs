use std::collections::BTreeMap;

use zeff_emu_common::replay::{ReplayEvent, ReplayStartMetadata};

use super::*;
use crate::emu_backend::loader::{DirectNesTasExecutionLoader, direct_nes_tas_identity};
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasDigest, TasEditorExecutionAttachment, TasInitialBranch,
    TasProject,
};

pub(super) fn executable_state(
    frame_count: u64,
) -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    let root = crate::test_support::test_directory("tas-editor-ui-execution").unwrap();
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
    let identity = direct_nes_tas_identity(&backend, &rom, &start_state).unwrap();
    let project = TasProject::new(
        "ui-execution-project",
        identity,
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
    .unwrap();
    let manual = root.path().join("movie.ztas");
    project.save_atomic(&manual).unwrap();

    let mut state = TasEditorWindowState::with_seek_cache_root(root.path().join("seek-cache"));
    state.reduce(TasEditorAction::OpenProject(manual)).unwrap();
    let loader = DirectNesTasExecutionLoader::new(rom_path, Vec::new());
    state.attach_execution(TasEditorExecutionAttachment::Available(Box::new(loader)));
    assert!(state.execution_engine.is_some());
    (root, state)
}

#[test]
fn selected_input_playback_runs_through_that_input_row() {
    assert_eq!(execution_preview::selected_frame_playback_target(0, 3), 1);
    assert_eq!(execution_preview::selected_frame_playback_target(1, 3), 2);
    assert_eq!(execution_preview::selected_frame_playback_target(3, 3), 3);
    assert_eq!(
        execution_preview::selected_frame_playback_target(u64::MAX, 3),
        3
    );
}

#[test]
fn seek_and_single_frame_actions_present_only_the_private_exact_framebuffer() {
    let (_root, mut state) = executable_state(3);

    state.reduce(TasEditorAction::ExecuteSeek(0)).unwrap();
    assert_eq!(state.session.as_ref().unwrap().cursor(), 0);
    assert_eq!(
        state.execution_preview.exact_frame().unwrap().rgba(),
        state
            .execution_engine
            .as_ref()
            .unwrap()
            .backend()
            .framebuffer()
    );

    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    assert_eq!(state.session.as_ref().unwrap().cursor(), 1);
    let exact_private = state
        .execution_engine
        .as_ref()
        .unwrap()
        .backend()
        .framebuffer()
        .to_vec();
    assert_eq!(
        state.execution_preview.exact_frame().unwrap().rgba(),
        exact_private
    );

    state.reduce(TasEditorAction::ExecuteSeek(2)).unwrap();
    assert_eq!(state.session.as_ref().unwrap().cursor(), 2);
    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    assert_eq!(state.session.as_ref().unwrap().cursor(), 1);

    state.reduce(TasEditorAction::ExecuteSeek(3)).unwrap();
    assert_eq!(state.session.as_ref().unwrap().cursor(), 3);
}

#[test]
fn preview_texture_is_owned_by_each_egui_renderer_context() {
    let (_root, mut state) = executable_state(1);
    state.reduce(TasEditorAction::ExecuteSeek(0)).unwrap();

    for context in [egui::Context::default(), egui::Context::default()] {
        let _ = context.run_ui(Default::default(), |ui| {
            let mut actions = Vec::new();
            execution_preview::draw_execution_panel(
                ui,
                &TasEditorExecutionAvailability::Ready,
                0,
                1,
                &mut state.execution_preview,
                &mut actions,
            );
            assert!(actions.is_empty());
        });
    }

    assert_eq!(state.execution_preview.texture_count(), 2);
}

#[test]
fn drawn_preview_can_be_cleared_by_an_action_in_the_same_egui_frame() {
    let (_root, mut state) = executable_state(2);
    state.reduce(TasEditorAction::ExecuteSeek(0)).unwrap();
    let context = egui::Context::default();

    let _ = context.run_ui(Default::default(), |ui| {
        let mut actions = Vec::new();
        execution_preview::draw_execution_panel(
            ui,
            &TasEditorExecutionAvailability::Ready,
            0,
            2,
            &mut state.execution_preview,
            &mut actions,
        );
    });
    let texture_id = state.execution_preview.only_texture_id().unwrap();

    let output = context.run_ui(Default::default(), |ui| {
        let mut actions = Vec::new();
        execution_preview::draw_execution_panel(
            ui,
            &TasEditorExecutionAvailability::Ready,
            0,
            2,
            &mut state.execution_preview,
            &mut actions,
        );
        state.apply(TasEditorAction::SelectCursor(1));
    });

    assert!(frame_draws_texture(&context, &output, texture_id));
    assert!(output.textures_delta.free.contains(&texture_id));
    assert!(state.execution_preview.exact_frame().is_none());
}

#[test]
fn drawn_preview_can_be_replaced_by_an_action_in_the_same_egui_frame() {
    let (_root, mut state) = executable_state(2);
    state.reduce(TasEditorAction::ExecuteSeek(0)).unwrap();
    let context = egui::Context::default();

    let _ = context.run_ui(Default::default(), |ui| {
        let mut actions = Vec::new();
        execution_preview::draw_execution_panel(
            ui,
            &TasEditorExecutionAvailability::Ready,
            0,
            2,
            &mut state.execution_preview,
            &mut actions,
        );
    });
    let texture_id = state.execution_preview.only_texture_id().unwrap();

    let output = context.run_ui(Default::default(), |ui| {
        let mut actions = Vec::new();
        execution_preview::draw_execution_panel(
            ui,
            &TasEditorExecutionAvailability::Ready,
            0,
            2,
            &mut state.execution_preview,
            &mut actions,
        );
        state.apply(TasEditorAction::ExecuteSeek(1));
    });

    assert!(frame_draws_texture(&context, &output, texture_id));
    assert!(output.textures_delta.free.contains(&texture_id));
    assert!(state.execution_preview.exact_frame().is_some());
    assert_eq!(state.session.as_ref().unwrap().cursor(), 1);
}

fn frame_draws_texture(
    context: &egui::Context,
    output: &egui::FullOutput,
    texture_id: egui::TextureId,
) -> bool {
    context
        .tessellate(output.shapes.clone(), output.pixels_per_point)
        .iter()
        .any(|primitive| {
            matches!(
                &primitive.primitive,
                egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == texture_id
            )
        })
}

#[test]
fn selection_and_movie_edits_clear_a_private_preview_instead_of_showing_it_stale() {
    let (_root, mut state) = executable_state(2);
    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    assert!(state.execution_preview.exact_frame().is_some());

    state.reduce(TasEditorAction::SelectCursor(0)).unwrap();
    assert!(state.execution_preview.exact_frame().is_none());

    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 0,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    assert!(state.execution_preview.exact_frame().is_none());
}

#[test]
fn incompatible_autosave_recovery_detaches_private_execution() {
    let (_root, mut state) = executable_state(2);
    let session = state.session.as_ref().unwrap();
    let project = session.project();
    let mut identity = project.identity().clone();
    identity.source_media_sha256 = TasDigest([0xE1; 32]);
    let replacement = TasProject::new(
        project.project_id(),
        identity,
        project.start_state().to_vec(),
        project.replay_start().clone(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Recovered".to_owned(),
            frame_count: 2,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )
    .unwrap();
    let autosaves =
        TasAutosaveStore::beside_manual_save(session.manual_path(), TasAutosaveConfig::default())
            .unwrap();
    autosaves.save(&replacement).unwrap();

    let message = state
        .reduce(TasEditorAction::RecoverAutosave)
        .unwrap()
        .unwrap();

    assert!(state.execution_engine.is_none());
    assert!(state.execution_preview.exact_frame().is_none());
    assert!(message.contains("private execution detached"));
}

#[test]
fn redo_detaches_execution_when_history_restores_an_incompatible_movie() {
    let (_root, mut state) = executable_state(2);
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(|edit| {
            edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 1, side: 0 }])
        })
        .unwrap();

    state.reduce(TasEditorAction::Undo).unwrap();
    assert!(state.execution_engine.is_some());
    let message = state.reduce(TasEditorAction::Redo).unwrap().unwrap();

    assert!(state.execution_engine.is_none());
    assert!(message.contains("private execution detached"));
}
