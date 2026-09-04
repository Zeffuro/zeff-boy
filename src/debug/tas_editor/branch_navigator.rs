use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
struct BranchNavigatorEntry {
    id: String,
    name: String,
    origin: String,
    frame_count: u64,
    active: bool,
    root: bool,
    subtree_size: usize,
    contains_active: bool,
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    live_status: &TasEditorLiveStatus,
    recording_stopped: bool,
    active_branch_name: &mut String,
    actions: &mut Vec<TasEditorAction>,
) {
    let cursor = session.cursor();
    let can_create = recording_stopped && branch_creation_enabled(session, live_status);
    let can_rename = branch_rename_enabled(recording_stopped, live_status);
    ui.group(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Branches");
            ui.strong(format!("Editing {}", session.selected_branch().name()));
            ui.small(format!("{} branches", session.project().branches().len()));
        });
        ui.small(
            "Choose a timeline frame, then create a new future. The current branch is kept unchanged.",
        );
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    can_create,
                    egui::Button::new(format!("Create Branch at Frame {cursor}")),
                )
                .on_hover_text(branch_creation_hint(
                    session,
                    live_status,
                    recording_stopped,
                ))
                .clicked()
            {
                actions.push(generated_branch_action(session));
            }
            ui.small(branch_creation_status(
                session,
                live_status,
                recording_stopped,
            ));
        });
        ui.separator();
        ui.label("Active branch name");
        ui.horizontal_wrapped(|ui| {
            let name_changed = active_branch_name.trim() != session.selected_branch().name();
            ui.add_enabled(
                can_rename,
                egui::TextEdit::singleline(active_branch_name).desired_width(180.0),
            );
            if ui
                .add_enabled(
                    can_rename && !active_branch_name.trim().is_empty() && name_changed,
                    egui::Button::new("Rename"),
                )
                .on_hover_text("Rename the active branch without changing its movie")
                .clicked()
            {
                actions.push(TasEditorAction::RenameActiveBranch {
                    name: active_branch_name.clone(),
                });
            }
        });
        ui.small(
            "Root and active routes are kept. Delete removes the whole descendant subtree; Undo restores it.",
        );
        ui.separator();
        ui.label("Routes");
        egui::ScrollArea::vertical()
            .id_salt("tas_editor_branch_navigator")
            .max_height(120.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_enabled_ui(branch_selection_enabled(live_status), |ui| {
                    for entry in branch_entries(session) {
                        let marker = if entry.active { "● " } else { "" };
                        let label = format!(
                            "{marker}{} — {}; {} frames",
                            entry.name, entry.origin, entry.frame_count
                        );
                        ui.horizontal_wrapped(|ui| {
                            if ui.selectable_label(entry.active, label).clicked() {
                                actions.push(TasEditorAction::SelectBranch(entry.id.clone()));
                            }
                            if !entry.root {
                                let delete_label = if entry.subtree_size == 1 {
                                    "Delete".to_owned()
                                } else {
                                    format!("Delete subtree ({})", entry.subtree_size)
                                };
                                if ui
                                    .add_enabled(
                                        branch_deletion_enabled(recording_stopped, live_status)
                                            && !entry.contains_active,
                                        egui::Button::new(delete_label).small(),
                                    )
                                    .on_hover_text(branch_deletion_hint(
                                        &entry,
                                        recording_stopped,
                                        live_status,
                                    ))
                                    .clicked()
                                {
                                    actions.push(TasEditorAction::DeleteBranchSubtree {
                                        id: entry.id.clone(),
                                    });
                                }
                            }
                        });
                    }
                });
            });
    });
}

pub(super) fn draw_advanced_controls(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    live_status: &TasEditorLiveStatus,
    new_branch_id: &mut String,
    new_branch_name: &mut String,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.collapsing("Custom branch", |ui| {
        ui.small(format!(
            "Create a named branch from {} before input frame {}.",
            session.selected_branch().name(),
            session.cursor()
        ));
        if let Some(linked_cursor) = live_status.execution_boundary() {
            live_execution_ui::draw_linked_branch_controls(
                ui,
                linked_cursor,
                session.cursor(),
                new_branch_id,
                new_branch_name,
                actions,
            );
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.label("ID");
                ui.text_edit_singleline(new_branch_id);
                ui.label("Name");
                ui.text_edit_singleline(new_branch_name);
                if ui.button("Create and select").clicked() {
                    actions.push(TasEditorAction::ForkBranch {
                        id: new_branch_id.clone(),
                        name: live_execution_ui::branch_name_or_id(new_branch_id, new_branch_name),
                    });
                }
            });
        }
    });
}

pub(super) fn generated_branch_action(session: &TasEditorSession) -> TasEditorAction {
    let (id, name) = suggested_branch_identity(session);
    TasEditorAction::ForkBranch { id, name }
}

pub(super) fn branch_creation_enabled(
    session: &TasEditorSession,
    live_status: &TasEditorLiveStatus,
) -> bool {
    match live_status.execution_boundary() {
        Some(boundary) => session.cursor() == boundary,
        None => !live_status.locks_editor(),
    }
}

fn branch_entries(session: &TasEditorSession) -> Vec<BranchNavigatorEntry> {
    let mut entries = session
        .project()
        .branches()
        .iter()
        .map(|branch| {
            let subtree = branch_subtree_ids(session, branch.id());
            let root = branch.parent().is_none();
            let origin = branch.parent().map_or_else(
                || "root".to_owned(),
                |origin| {
                    let parent = session
                        .project()
                        .branch(&origin.branch_id)
                        .map_or(origin.branch_id.as_str(), |branch| branch.name());
                    format!("from {parent} at B({})", origin.fork_cursor)
                },
            );
            BranchNavigatorEntry {
                id: branch.id().to_owned(),
                name: branch.name().to_owned(),
                origin,
                frame_count: branch.frame_count(),
                active: branch.id() == session.selected_branch_id(),
                root,
                subtree_size: subtree.len(),
                contains_active: subtree.contains(session.selected_branch_id()),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .root
            .cmp(&left.root)
            .then_with(|| left.id.cmp(&right.id))
    });
    entries
}

fn suggested_branch_identity(session: &TasEditorSession) -> (String, String) {
    let mut number = 1_u64;
    loop {
        let id = format!("branch-{number}");
        if session.project().branch(&id).is_none() {
            return (id, format!("Branch {number}"));
        }
        number += 1;
    }
}

fn branch_selection_enabled(live_status: &TasEditorLiveStatus) -> bool {
    !live_status.holds_authority()
}

fn branch_rename_enabled(recording_stopped: bool, live_status: &TasEditorLiveStatus) -> bool {
    recording_stopped && branch_selection_enabled(live_status)
}

fn branch_deletion_enabled(recording_stopped: bool, live_status: &TasEditorLiveStatus) -> bool {
    recording_stopped && !live_status.holds_authority()
}

fn branch_subtree_ids(session: &TasEditorSession, branch_id: &str) -> BTreeSet<String> {
    let mut subtree = BTreeSet::from([branch_id.to_owned()]);
    loop {
        let before = subtree.len();
        for branch in session.project().branches() {
            if branch
                .parent()
                .is_some_and(|parent| subtree.contains(&parent.branch_id))
            {
                subtree.insert(branch.id().to_owned());
            }
        }
        if subtree.len() == before {
            return subtree;
        }
    }
}

fn branch_deletion_hint(
    entry: &BranchNavigatorEntry,
    recording_stopped: bool,
    live_status: &TasEditorLiveStatus,
) -> &'static str {
    if !recording_stopped {
        "Stop recording before deleting a branch"
    } else if live_status.holds_authority() {
        "Finish the live game decision before deleting a branch"
    } else if entry.active {
        "Switch to another branch before deleting this one"
    } else if entry.contains_active {
        "Switch outside this branch family before deleting it"
    } else {
        "Delete this branch and all of its descendants"
    }
}

fn branch_creation_hint(
    session: &TasEditorSession,
    live_status: &TasEditorLiveStatus,
    recording_stopped: bool,
) -> &'static str {
    if !recording_stopped {
        "Stop recording before creating a branch"
    } else if live_status
        .execution_boundary()
        .is_some_and(|boundary| boundary != session.cursor())
    {
        "Move the loaded game to the selected frame before creating a branch"
    } else {
        "Create and select a branch at the current frame"
    }
}

fn branch_creation_status(
    session: &TasEditorSession,
    live_status: &TasEditorLiveStatus,
    recording_stopped: bool,
) -> String {
    if !recording_stopped {
        return "Stop recording before creating a branch.".to_owned();
    }
    if let Some(boundary) = live_status.execution_boundary()
        && boundary != session.cursor()
    {
        return format!(
            "Move the loaded game to before input frame {} to branch here.",
            session.cursor()
        );
    }
    format!(
        "Ready: the new branch will start before input frame {}.",
        session.cursor()
    )
}

#[cfg(test)]
mod tests {
    use super::{
        branch_creation_enabled, branch_creation_status, branch_deletion_enabled, branch_entries,
        branch_rename_enabled, branch_selection_enabled, generated_branch_action,
    };
    use crate::debug::TasEditorLiveStatus;

    use super::super::tests::state_with_project;

    #[test]
    fn navigator_orders_root_then_branch_id_and_labels_each_origin() {
        let (_root, mut state) = state_with_project(4);
        state.session.as_mut().unwrap().set_cursor(2).unwrap();
        state
            .reduce(crate::debug::tas_editor::TasEditorAction::ForkBranch {
                id: "z-route".to_owned(),
                name: "Z route".to_owned(),
            })
            .unwrap();
        state
            .reduce(crate::debug::tas_editor::TasEditorAction::SelectBranch(
                "main".to_owned(),
            ))
            .unwrap();
        state
            .reduce(crate::debug::tas_editor::TasEditorAction::ForkBranch {
                id: "a-route".to_owned(),
                name: "A route".to_owned(),
            })
            .unwrap();

        let entries = branch_entries(state.session.as_ref().unwrap());
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.id.as_str(), entry.origin.as_str(), entry.active))
                .collect::<Vec<_>>(),
            vec![
                ("main", "root", false),
                ("a-route", "from Main at B(2)", true),
                ("z-route", "from Main at B(2)", false),
            ]
        );
    }

    #[test]
    fn generated_branch_action_uses_the_first_available_identity() {
        let (_root, mut state) = state_with_project(4);
        assert_eq!(
            generated_branch_action(state.session.as_ref().unwrap()),
            crate::debug::tas_editor::TasEditorAction::ForkBranch {
                id: "branch-1".to_owned(),
                name: "Branch 1".to_owned(),
            }
        );
        state
            .reduce(generated_branch_action(state.session.as_ref().unwrap()))
            .unwrap();
        assert_eq!(
            generated_branch_action(state.session.as_ref().unwrap()),
            crate::debug::tas_editor::TasEditorAction::ForkBranch {
                id: "branch-2".to_owned(),
                name: "Branch 2".to_owned(),
            }
        );
    }

    #[test]
    fn navigator_keeps_branch_selection_and_creation_at_the_existing_authority_gates() {
        let (_root, mut state) = state_with_project(4);
        state.session.as_mut().unwrap().set_cursor(2).unwrap();
        let ready = TasEditorLiveStatus::Ready {
            recording_available: true,
        };
        assert!(branch_selection_enabled(&ready));
        assert!(branch_rename_enabled(true, &ready));
        assert!(branch_deletion_enabled(true, &ready));
        assert!(branch_creation_enabled(
            state.session.as_ref().unwrap(),
            &ready
        ));

        let linked_elsewhere = TasEditorLiveStatus::Linked {
            cursor: 1,
            recording_available: true,
        };
        assert!(!branch_selection_enabled(&linked_elsewhere));
        assert!(!branch_rename_enabled(true, &linked_elsewhere));
        assert!(!branch_rename_enabled(false, &ready));
        assert!(!branch_deletion_enabled(true, &linked_elsewhere));
        assert!(!branch_deletion_enabled(false, &ready));
        assert!(!branch_creation_enabled(
            state.session.as_ref().unwrap(),
            &linked_elsewhere
        ));
        assert_eq!(
            branch_creation_status(state.session.as_ref().unwrap(), &linked_elsewhere, true),
            "Move the loaded game to before input frame 2 to branch here."
        );
    }

    #[test]
    fn navigator_counts_descendants_and_protects_the_active_family() {
        let (_root, mut state) = state_with_project(4);
        state.session.as_mut().unwrap().set_cursor(2).unwrap();
        state
            .reduce(crate::debug::tas_editor::TasEditorAction::ForkBranch {
                id: "child".to_owned(),
                name: "Child".to_owned(),
            })
            .unwrap();
        state
            .reduce(crate::debug::tas_editor::TasEditorAction::ForkBranch {
                id: "grandchild".to_owned(),
                name: "Grandchild".to_owned(),
            })
            .unwrap();

        let entries = branch_entries(state.session.as_ref().unwrap());
        assert_eq!(
            entries
                .iter()
                .map(|entry| { (entry.id.as_str(), entry.subtree_size, entry.contains_active,) })
                .collect::<Vec<_>>(),
            vec![
                ("main", 3, true),
                ("child", 2, true),
                ("grandchild", 1, true),
            ]
        );
    }
}
