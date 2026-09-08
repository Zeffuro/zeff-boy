use crate::settings::Settings;

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.label(
        egui::RichText::new("Recovery preferences are global to this installation.")
            .small()
            .weak(),
    );

    ui.separator();
    ui.heading("Recovery state");
    let recovery_save = ui.checkbox(
        &mut settings.emulation.save_recovery_state,
        "Save recovery state when stopping",
    );
    #[cfg(target_arch = "wasm32")]
    recovery_save.on_hover_text(
        "Keep this page open until saving finishes; abruptly closing it can interrupt browser storage.",
    );
    #[cfg(not(target_arch = "wasm32"))]
    let _ = recovery_save;
    ui.checkbox(
        &mut settings.emulation.resume_recovery_state,
        "Resume fresh recovery state automatically",
    );
    if settings.emulation.recovery_migration_notice_pending {
        ui.group(|ui| {
            ui.label("Automatic save and resume are now separate settings.");
            ui.horizontal_wrapped(|ui| {
                if ui.button("Keep automatic resume").clicked() {
                    settings.emulation.resume_recovery_state = true;
                    settings.emulation.recovery_migration_notice_pending = false;
                }
                if ui.button("Keep resume off").clicked() {
                    settings.emulation.resume_recovery_state = false;
                    settings.emulation.recovery_migration_notice_pending = false;
                }
            });
        });
    }
}
