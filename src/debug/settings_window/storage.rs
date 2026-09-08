use crate::settings::Settings;

use super::{
    layout::{checkbox, helper, row},
    search::{self, SettingId as Id},
};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut super::SettingsUiState) {
    draw_settings_files(ui, settings, state);
    helper(
        ui,
        egui::RichText::new("Recovery preferences are global to this installation.")
            .small()
            .weak(),
    );
    ui.add_space(14.0);
    ui.heading("Recovery state");
    let recovery_save = checkbox(
        ui,
        Id::StorageSaveRecoveryStateWhenStopping,
        None,
        &mut settings.emulation.save_recovery_state,
    );
    #[cfg(target_arch = "wasm32")]
    recovery_save.on_hover_text("Keep this page open until saving finishes; abruptly closing it can interrupt browser storage.");
    #[cfg(not(target_arch = "wasm32"))]
    let _ = recovery_save;
    checkbox(
        ui,
        Id::StorageResumeFreshRecoveryStateAutomatically,
        None,
        &mut settings.emulation.resume_recovery_state,
    );
    for id in [Id::StorageKeepAutomaticResume, Id::StorageKeepResumeOff] {
        search::conditional(
            ui,
            id,
            Id::StorageResumeFreshRecoveryStateAutomatically,
            settings.emulation.recovery_migration_notice_pending,
            "This migration choice is only available when an existing recovery preference needs review.",
        );
    }
    if settings.emulation.recovery_migration_notice_pending {
        ui.group(|ui| {
            helper(ui, "Automatic save and resume are now separate settings.");
            if row(ui, Id::StorageKeepAutomaticResume, None, |ui| {
                ui.button("Keep automatic resume")
            })
            .clicked()
            {
                settings.emulation.resume_recovery_state = true;
                settings.emulation.recovery_migration_notice_pending = false;
            }
            if row(ui, Id::StorageKeepResumeOff, None, |ui| {
                ui.button("Keep resume off")
            })
            .clicked()
            {
                settings.emulation.resume_recovery_state = false;
                settings.emulation.recovery_migration_notice_pending = false;
            }
        });
    }
}

fn draw_settings_files(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut super::SettingsUiState,
) {
    ui.heading("Settings file");
    helper(
        ui,
        "Export includes current unsaved preferences, profiles and device settings. It can contain local file paths.",
    );
    if row(ui, Id::StorageExportSettings, None, |ui| {
        ui.button("Export settings…")
    })
    .clicked()
    {
        state.settings_file_notice = Some(match export_settings(settings) {
            Ok(true) => "Settings exported.".to_owned(),
            Ok(false) => "Export canceled.".to_owned(),
            Err(error) => format!("Could not export settings: {error:#}"),
        });
    }
    if row(ui, Id::StorageImportSettings, None, |ui| {
        ui.button("Choose settings file…")
    })
    .clicked()
    {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = crate::platform::FileDialog::new()
            .add_filter("Settings", &["json"])
            .set_title("Import settings")
            .pick_file()
        {
            let result = std::fs::metadata(&path)
                .map_err(anyhow::Error::from)
                .and_then(|metadata| {
                    if metadata.len() > 4 * 1024 * 1024 {
                        anyhow::bail!("Settings file exceeds 4 MiB");
                    }
                    Ok(std::fs::read_to_string(path)?)
                });
            match result {
                Ok(json) => state.settings_import_json = Some(json),
                Err(error) => {
                    state.settings_file_notice = Some(format!("Could not read settings: {error:#}"))
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        crate::platform::FileDialog::new()
            .add_filter("Settings", &["json"])
            .set_title("Import settings")
            .pick_file_web(state.settings_import_file.clone());
    }
    #[cfg(target_arch = "wasm32")]
    if let Some((_, bytes)) = state.settings_import_file.borrow_mut().take() {
        if bytes.len() > 4 * 1024 * 1024 {
            state.settings_file_notice = Some("Settings file exceeds 4 MiB.".to_owned());
        } else {
            match String::from_utf8(bytes) {
                Ok(json) => state.settings_import_json = Some(json),
                Err(_) => {
                    state.settings_file_notice =
                        Some("Settings must be a UTF-8 JSON file.".to_owned())
                }
            }
        }
    }
    if state.settings_import_json.is_some() {
        ui.group(|ui| {
            ui.label("Replace current preferences, profiles and device assignments with this file? Export first to keep your current setup.");
            ui.horizontal_wrapped(|ui| {
                if ui.button("Import and replace").clicked() {
                    let json = state.settings_import_json.take().expect("pending settings import");
                    state.settings_file_notice = Some(match settings.import_settings_json(&json) {
                        Ok(()) => "Imported settings. Check the save status above before closing.".to_owned(),
                        Err(error) => format!("Could not import settings: {error:#}"),
                    });
                }
                if ui.button("Cancel import").clicked() {
                    state.settings_import_json = None;
                }
            });
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        let status = crate::platform::settings_storage_status();
        search::conditional(
            ui,
            Id::StorageLoadSavedSettings,
            Id::StorageExportSettings,
            status.conflicting_json.is_some(),
            "Available when another tab has changed the saved settings.",
        );
        if status.conflicting_json.is_some() {
            helper(
                ui,
                "Another tab saved different settings. Export local changes before loading that saved version.",
            );
            if row(ui, Id::StorageLoadSavedSettings, None, |ui| {
                ui.add_enabled(
                    status.pending == 0,
                    egui::Button::new("Load saved settings (discard local edits)"),
                )
            })
            .clicked()
            {
                state.settings_file_notice = Some(match settings.accept_browser_settings() {
                    Ok(()) => "Loaded the saved settings from the other tab.".to_owned(),
                    Err(error) => format!("Could not load saved settings: {error:#}"),
                });
            }
        }
    }
    if let Some(notice) = &state.settings_file_notice {
        ui.label(notice);
    }
    ui.add_space(12.0);
}

pub(super) fn export_settings(settings: &Settings) -> anyhow::Result<bool> {
    let json = settings.export_settings_json()?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(path) = crate::platform::FileDialog::new()
            .add_filter("Settings", &["json"])
            .set_title("Export settings")
            .set_file_name("zeff-boy-settings.json")
            .save_file()
        else {
            return Ok(false);
        };
        crate::platform::write_file_atomically(&path, json.as_bytes())?;
    }
    #[cfg(target_arch = "wasm32")]
    crate::platform::download_file("zeff-boy-settings.json", json.as_bytes());
    Ok(true)
}
