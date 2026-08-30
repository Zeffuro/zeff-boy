use std::path::PathBuf;

use anyhow::{Result, bail};

use super::{TasEditorAction, TasEditorWindowState};
use crate::tas_project::{TasAnnotation, TasDigest, TasEditorSession, TasMarker};

const LIST_ROW_HEIGHT: f32 = 22.0;

#[derive(Clone, Copy)]
struct MetadataDrawContext<'a> {
    branch_id: &'a str,
    frame_count: u64,
    cursor: u64,
    project_sha256: TasDigest,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum TasMetadataMutation {
    UpsertMarker {
        original_id: Option<String>,
        marker: TasMarker,
    },
    RemoveMarker {
        branch_id: String,
        id: String,
    },
    UpsertAnnotation {
        original_id: Option<String>,
        annotation: TasAnnotation,
    },
    RemoveAnnotation {
        branch_id: String,
        id: String,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct TasMetadataAction {
    expected_project_sha256: TasDigest,
    mutation: TasMetadataMutation,
}

impl TasMetadataAction {
    pub(super) fn new(expected_project_sha256: TasDigest, mutation: TasMetadataMutation) -> Self {
        Self {
            expected_project_sha256,
            mutation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EditorContext {
    manual_path: PathBuf,
    project_id: String,
    branch_id: String,
    edit_generation: u64,
}

#[derive(Default)]
struct MarkerDraft {
    original_id: Option<String>,
    id: String,
    name: String,
    cursor: u64,
}

#[derive(Default)]
struct AnnotationDraft {
    original_id: Option<String>,
    id: String,
    kind: String,
    text: String,
    start: u64,
    length: u64,
}

pub(super) struct TasMetadataEditorState {
    context: Option<EditorContext>,
    marker_indices: Vec<usize>,
    annotation_indices: Vec<usize>,
    marker: MarkerDraft,
    annotation: AnnotationDraft,
}

impl TasMetadataEditorState {
    pub(super) fn new() -> Self {
        Self {
            context: None,
            marker_indices: Vec::new(),
            annotation_indices: Vec::new(),
            marker: MarkerDraft::default(),
            annotation: AnnotationDraft::default(),
        }
    }

    pub(super) fn clear(&mut self) {
        self.context = None;
        self.marker_indices.clear();
        self.annotation_indices.clear();
        self.marker = MarkerDraft::default();
        self.annotation = AnnotationDraft::default();
    }

    fn sync_context(&mut self, session: &TasEditorSession) {
        let context = EditorContext {
            manual_path: session.manual_path().to_path_buf(),
            project_id: session.project().project_id().to_owned(),
            branch_id: session.selected_branch_id().to_owned(),
            edit_generation: session.project().edit_generation(),
        };
        if self.context.as_ref() == Some(&context) {
            return;
        }
        self.marker_indices = session
            .project()
            .markers()
            .iter()
            .enumerate()
            .filter_map(|(index, marker)| (marker.branch_id == context.branch_id).then_some(index))
            .collect();
        self.annotation_indices = session
            .project()
            .annotations()
            .iter()
            .enumerate()
            .filter_map(|(index, annotation)| {
                (annotation.branch_id == context.branch_id).then_some(index)
            })
            .collect();
        self.marker = MarkerDraft {
            cursor: session.cursor(),
            ..MarkerDraft::default()
        };
        self.annotation = AnnotationDraft {
            kind: "note".to_owned(),
            start: session.cursor(),
            length: 1,
            ..AnnotationDraft::default()
        };
        self.context = Some(context);
    }
}

impl TasEditorWindowState {
    pub(super) fn apply_metadata_action(&mut self, action: TasMetadataAction) -> Result<String> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        if session.project_content_sha256() != action.expected_project_sha256 {
            bail!("TAS project changed after this marker/annotation edit was drafted; retry it");
        }
        let selected_branch = session.selected_branch_id().to_owned();
        let (label, id, outcome) = match action.mutation {
            TasMetadataMutation::UpsertMarker {
                original_id,
                marker,
            } => {
                ensure_selected_branch(&selected_branch, &marker.branch_id)?;
                let mut markers = session.project().markers().to_vec();
                let label = if let Some(original_id) = original_id {
                    let index = find_marker(&markers, &marker.branch_id, &original_id)?;
                    markers[index] = marker.clone();
                    "Updated marker"
                } else {
                    markers.push(marker.clone());
                    "Added marker"
                };
                let outcome = session.edit_transaction(move |edit| {
                    edit.replace_markers(markers);
                    Ok(())
                })?;
                (label, marker.id, outcome)
            }
            TasMetadataMutation::RemoveMarker { branch_id, id } => {
                ensure_selected_branch(&selected_branch, &branch_id)?;
                let mut markers = session.project().markers().to_vec();
                let index = find_marker(&markers, &branch_id, &id)?;
                markers.remove(index);
                let outcome = session.edit_transaction(move |edit| {
                    edit.replace_markers(markers);
                    Ok(())
                })?;
                ("Removed marker", id, outcome)
            }
            TasMetadataMutation::UpsertAnnotation {
                original_id,
                annotation,
            } => {
                ensure_selected_branch(&selected_branch, &annotation.branch_id)?;
                let mut annotations = session.project().annotations().to_vec();
                let label = if let Some(original_id) = original_id {
                    let index = find_annotation(&annotations, &annotation.branch_id, &original_id)?;
                    annotations[index] = annotation.clone();
                    "Updated annotation"
                } else {
                    annotations.push(annotation.clone());
                    "Added annotation"
                };
                let outcome = session.edit_transaction(move |edit| {
                    edit.replace_annotations(annotations);
                    Ok(())
                })?;
                (label, annotation.id, outcome)
            }
            TasMetadataMutation::RemoveAnnotation { branch_id, id } => {
                ensure_selected_branch(&selected_branch, &branch_id)?;
                let mut annotations = session.project().annotations().to_vec();
                let index = find_annotation(&annotations, &branch_id, &id)?;
                annotations.remove(index);
                let outcome = session.edit_transaction(move |edit| {
                    edit.replace_annotations(annotations);
                    Ok(())
                })?;
                ("Removed annotation", id, outcome)
            }
        };

        if outcome.changed {
            self.execution_preview.clear();
            self.metadata_editor.clear();
            Ok(format!("{label} {id:?}"))
        } else {
            Ok(format!("No marker/annotation change for {id:?}"))
        }
    }
}

pub(super) fn draw_metadata_editor(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasMetadataEditorState,
    actions: &mut Vec<TasEditorAction>,
) {
    state.sync_context(session);
    let context = MetadataDrawContext {
        branch_id: session.selected_branch_id(),
        frame_count: session.selected_branch().frame_count(),
        cursor: session.cursor(),
        project_sha256: session.project_content_sha256(),
    };

    ui.collapsing(format!("Markers ({})", state.marker_indices.len()), |ui| {
        draw_marker_form(ui, context, state, actions);
        draw_marker_list(ui, session, context, state, actions);
    });
    ui.collapsing(
        format!("Annotations ({})", state.annotation_indices.len()),
        |ui| {
            draw_annotation_form(ui, context, state, actions);
            draw_annotation_list(ui, session, context, state, actions);
        },
    );
}

fn draw_marker_form(
    ui: &mut egui::Ui,
    context: MetadataDrawContext<'_>,
    state: &mut TasMetadataEditorState,
    actions: &mut Vec<TasEditorAction>,
) {
    egui::Grid::new("tas_marker_editor").show(ui, |ui| {
        ui.label("ID");
        ui.text_edit_singleline(&mut state.marker.id);
        ui.end_row();
        ui.label("Name");
        ui.text_edit_singleline(&mut state.marker.name);
        ui.end_row();
        ui.label("Cursor");
        ui.add(egui::DragValue::new(&mut state.marker.cursor).range(0..=context.frame_count));
        if ui.button("Use selected").clicked() {
            state.marker.cursor = context.cursor;
        }
        ui.end_row();
    });
    ui.horizontal(|ui| {
        let label = if state.marker.original_id.is_some() {
            "Update marker"
        } else {
            "Add marker"
        };
        if ui.button(label).clicked() {
            actions.push(TasEditorAction::Metadata(TasMetadataAction::new(
                context.project_sha256,
                TasMetadataMutation::UpsertMarker {
                    original_id: state.marker.original_id.clone(),
                    marker: TasMarker {
                        id: state.marker.id.trim().to_owned(),
                        branch_id: context.branch_id.to_owned(),
                        cursor: state.marker.cursor,
                        name: state.marker.name.clone(),
                    },
                },
            )));
        }
        if state.marker.original_id.is_some() && ui.button("Cancel edit").clicked() {
            state.marker = MarkerDraft {
                cursor: context.cursor,
                ..MarkerDraft::default()
            };
        }
    });
}

fn draw_marker_list(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    context: MetadataDrawContext<'_>,
    state: &mut TasMetadataEditorState,
    actions: &mut Vec<TasEditorAction>,
) {
    let markers = session.project().markers();
    let mut edit = None;
    egui::ScrollArea::vertical()
        .id_salt("tas_marker_list")
        .max_height(154.0)
        .show_rows(
            ui,
            LIST_ROW_HEIGHT,
            state.marker_indices.len(),
            |ui, rows| {
                for row in rows {
                    let marker = &markers[state.marker_indices[row]];
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{:>8}", marker.cursor));
                        ui.label(short_text(&marker.name));
                        ui.small(format!("[{}]", marker.id));
                        if ui.small_button("Edit").clicked() {
                            edit = Some(marker.clone());
                        }
                        if ui.small_button("Remove").clicked() {
                            actions.push(TasEditorAction::Metadata(TasMetadataAction::new(
                                context.project_sha256,
                                TasMetadataMutation::RemoveMarker {
                                    branch_id: marker.branch_id.clone(),
                                    id: marker.id.clone(),
                                },
                            )));
                        }
                    });
                }
            },
        );
    if let Some(marker) = edit {
        state.marker = MarkerDraft {
            original_id: Some(marker.id.clone()),
            id: marker.id,
            name: marker.name,
            cursor: marker.cursor,
        };
    }
}

fn draw_annotation_form(
    ui: &mut egui::Ui,
    context: MetadataDrawContext<'_>,
    state: &mut TasMetadataEditorState,
    actions: &mut Vec<TasEditorAction>,
) {
    egui::Grid::new("tas_annotation_editor").show(ui, |ui| {
        ui.label("ID");
        ui.text_edit_singleline(&mut state.annotation.id);
        ui.end_row();
        ui.label("Kind");
        ui.text_edit_singleline(&mut state.annotation.kind);
        ui.end_row();
        ui.label("Text");
        ui.text_edit_multiline(&mut state.annotation.text);
        ui.end_row();
        ui.label("Start");
        ui.add(egui::DragValue::new(&mut state.annotation.start).range(0..=context.frame_count));
        if ui.button("Use selected").clicked() {
            state.annotation.start = context.cursor;
        }
        ui.end_row();
        let max_length = context
            .frame_count
            .saturating_sub(state.annotation.start)
            .max(1);
        ui.label("Length");
        ui.add(egui::DragValue::new(&mut state.annotation.length).range(1..=max_length));
        ui.end_row();
    });
    let can_save = state.annotation.start < context.frame_count;
    ui.horizontal(|ui| {
        let label = if state.annotation.original_id.is_some() {
            "Update annotation"
        } else {
            "Add annotation"
        };
        if ui.add_enabled(can_save, egui::Button::new(label)).clicked() {
            actions.push(TasEditorAction::Metadata(TasMetadataAction::new(
                context.project_sha256,
                TasMetadataMutation::UpsertAnnotation {
                    original_id: state.annotation.original_id.clone(),
                    annotation: TasAnnotation {
                        id: state.annotation.id.trim().to_owned(),
                        branch_id: context.branch_id.to_owned(),
                        start: state.annotation.start,
                        length: state.annotation.length,
                        kind: state.annotation.kind.trim().to_owned(),
                        text: state.annotation.text.clone(),
                    },
                },
            )));
        }
        if !can_save {
            ui.small("Select a cursor before the branch end to create an annotation.");
        }
        if state.annotation.original_id.is_some() && ui.button("Cancel edit").clicked() {
            state.annotation = AnnotationDraft {
                kind: "note".to_owned(),
                start: context.cursor,
                length: 1,
                ..AnnotationDraft::default()
            };
        }
    });
}

fn draw_annotation_list(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    context: MetadataDrawContext<'_>,
    state: &mut TasMetadataEditorState,
    actions: &mut Vec<TasEditorAction>,
) {
    let annotations = session.project().annotations();
    let mut edit = None;
    egui::ScrollArea::vertical()
        .id_salt("tas_annotation_list")
        .max_height(154.0)
        .show_rows(
            ui,
            LIST_ROW_HEIGHT,
            state.annotation_indices.len(),
            |ui, rows| {
                for row in rows {
                    let annotation = &annotations[state.annotation_indices[row]];
                    ui.horizontal(|ui| {
                        ui.monospace(format!(
                            "{:>8}..{}",
                            annotation.start,
                            annotation.start.saturating_add(annotation.length)
                        ));
                        ui.label(short_text(&annotation.text));
                        ui.small(format!("{} [{}]", annotation.kind, annotation.id));
                        if ui.small_button("Edit").clicked() {
                            edit = Some(annotation.clone());
                        }
                        if ui.small_button("Remove").clicked() {
                            actions.push(TasEditorAction::Metadata(TasMetadataAction::new(
                                context.project_sha256,
                                TasMetadataMutation::RemoveAnnotation {
                                    branch_id: annotation.branch_id.clone(),
                                    id: annotation.id.clone(),
                                },
                            )));
                        }
                    });
                }
            },
        );
    if let Some(annotation) = edit {
        state.annotation = AnnotationDraft {
            original_id: Some(annotation.id.clone()),
            id: annotation.id,
            kind: annotation.kind,
            text: annotation.text,
            start: annotation.start,
            length: annotation.length,
        };
    }
}

fn ensure_selected_branch(selected: &str, action_branch: &str) -> Result<()> {
    if selected != action_branch {
        bail!("TAS branch selection changed; retry the marker/annotation edit");
    }
    Ok(())
}

fn find_marker(markers: &[TasMarker], branch_id: &str, id: &str) -> Result<usize> {
    markers
        .iter()
        .position(|marker| marker.branch_id == branch_id && marker.id == id)
        .ok_or_else(|| anyhow::anyhow!("TAS marker {id:?} no longer exists on this branch"))
}

fn find_annotation(annotations: &[TasAnnotation], branch_id: &str, id: &str) -> Result<usize> {
    annotations
        .iter()
        .position(|annotation| annotation.branch_id == branch_id && annotation.id == id)
        .ok_or_else(|| anyhow::anyhow!("TAS annotation {id:?} no longer exists on this branch"))
}

fn short_text(text: &str) -> String {
    let mut chars = text.chars();
    let short = chars.by_ref().take(64).collect::<String>();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}
