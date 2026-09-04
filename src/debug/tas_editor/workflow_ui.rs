use super::{TasEditorAction, TasEditorExecutionAvailability, TasEditorFileRequest};

pub(super) fn draw_pending_file_request(
    ui: &egui::Ui,
    request: Option<TasEditorFileRequest>,
    action_allowed: bool,
    actions: &mut Vec<TasEditorAction>,
) {
    let Some(request) = request else {
        return;
    };
    let destination = match request {
        TasEditorFileRequest::LoadGame => "loading another game",
        TasEditorFileRequest::OpenProject => "opening another TAS project",
        TasEditorFileRequest::NewProject => "creating another TAS project",
        TasEditorFileRequest::NewGameGearNoSaveProject => "creating another TAS project",
        TasEditorFileRequest::ImportReplay => "importing a replay as a TAS project",
        TasEditorFileRequest::ExportReplay => "exporting the TAS project as a replay",
    };
    egui::Window::new("Unsaved TAS changes")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ui.ctx(), |ui| {
            ui.label(format!("Save the current project before {destination}?"));
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(action_allowed, egui::Button::new("Save and continue"))
                    .clicked()
                {
                    actions.push(TasEditorAction::ContinueFileRequest { save: true });
                }
                if ui
                    .add_enabled(action_allowed, egui::Button::new("Continue without saving"))
                    .clicked()
                {
                    actions.push(TasEditorAction::ContinueFileRequest { save: false });
                }
                if ui
                    .add_enabled(action_allowed, egui::Button::new("Cancel"))
                    .clicked()
                {
                    actions.push(TasEditorAction::CancelFileRequest);
                }
            });
        });
}

pub(super) fn draw_game_gear_no_save_confirmation(ui: &egui::Ui, pending: bool) -> Option<bool> {
    if !pending {
        return None;
    }
    let mut response = None;
    egui::Window::new("Confirm Game Gear cartridge memory")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ui.ctx(), |ui| {
            ui.label("This Game Gear cartridge has no verified board record.");
            ui.label(
                "Create a TAS only if you have confirmed that it has no cartridge save memory.",
            );
            ui.small(
                "This choice is saved in the project. Battery-backed cartridges are not supported by this profile.",
            );
            ui.horizontal(|ui| {
                if ui.button("I confirm: no cartridge save memory").clicked() {
                    response = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    response = Some(false);
                }
            });
        });
    response
}

pub(super) fn draw_autosave_recovery_confirmation(
    ui: &egui::Ui,
    pending: bool,
    action_allowed: bool,
    actions: &mut Vec<TasEditorAction>,
) {
    if !pending {
        return;
    }
    egui::Window::new("Recover TAS autosave?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ui.ctx(), |ui| {
            ui.label(
                "Recovery replaces the current in-memory timeline and clears its undo history.",
            );
            ui.label("The manually saved .ztas file is not changed until you save again.");
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(action_allowed, egui::Button::new("Recover newest copy"))
                    .clicked()
                {
                    actions.push(TasEditorAction::RecoverAutosave);
                }
                if ui
                    .add_enabled(action_allowed, egui::Button::new("Cancel"))
                    .clicked()
                {
                    actions.push(TasEditorAction::CancelAutosaveRecovery);
                }
            });
        });
}

pub(super) fn draw_project_replacement_confirmation(
    ui: &egui::Ui,
    path: Option<&std::path::Path>,
) -> Option<bool> {
    let path = path?;
    let mut response = None;
    egui::Window::new("Replace TAS project?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ui.ctx(), |ui| {
            ui.label(format!("{} already exists.", path.display()));
            ui.label("Replace it with a new TAS project?");
            ui.small("The existing valid project will be kept as a .bak file.");
            ui.horizontal(|ui| {
                if ui.button("Replace project").clicked() {
                    response = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    response = Some(false);
                }
            });
        });
    response
}

pub(super) fn draw_empty_project_state(
    ui: &mut egui::Ui,
    availability: &TasEditorExecutionAvailability,
    action_allowed: bool,
    file_request: &mut Option<TasEditorFileRequest>,
) {
    ui.separator();
    ui.group(|ui| {
        ui.heading("Start a TAS project");
        ui.label("A TAS project stores the controller input for each emulated frame.");

        match availability {
            TasEditorExecutionAvailability::GameReady => {
                ui.label("A compatible direct cartridge is loaded and ready.");
                if ui
                    .add_enabled(
                        action_allowed,
                        egui::Button::new("New TAS from Loaded Game…"),
                    )
                    .clicked()
                {
                    *file_request = Some(TasEditorFileRequest::NewProject);
                }
            }
            TasEditorExecutionAvailability::Checking => {
                ui.label("Checking the game loaded in the main emulator…");
            }
            TasEditorExecutionAvailability::Unavailable(reason) => {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!("A compatible game is not ready: {reason}"),
                );
                if ui
                    .add_enabled(action_allowed, egui::Button::new("Load a game…"))
                    .clicked()
                {
                    *file_request = Some(TasEditorFileRequest::LoadGame);
                }
            }
            TasEditorExecutionAvailability::Ready => {
                ui.label("A compatible game is loaded and ready.");
            }
        }

        if ui
            .add_enabled(action_allowed, egui::Button::new("Open an existing .ztas…"))
            .clicked()
        {
            *file_request = Some(TasEditorFileRequest::OpenProject);
        }
        if matches!(availability, TasEditorExecutionAvailability::GameReady)
            && ui
                .add_enabled(action_allowed, egui::Button::new("Import .zrpl as TAS…"))
                .clicked()
        {
            *file_request = Some(TasEditorFileRequest::ImportReplay);
        }
        ui.separator();
        ui.small(
            "Open a project, then connect it to the loaded game to record or replay its timeline.",
        );
    });
}
