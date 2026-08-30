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
                ui.label("A compatible NES cartridge is loaded and ready.");
                if ui
                    .add_enabled(
                        action_allowed,
                        egui::Button::new("Create TAS from loaded game…"),
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
        ui.separator();
        ui.small("Open a project, then link its timeline to the loaded game to play or record.");
    });
}
