use super::MenuAction;
use crate::settings::Settings;
use std::path::PathBuf;

pub(super) fn draw(
    ui: &mut egui::Ui,
    actions: &mut Vec<MenuAction>,
    settings: &Settings,
    slot_labels: &[String; 10],
    is_recording_audio: bool,
    is_recording_replay: bool,
    is_playing_replay: bool,
) {
    if ui.button("Open").clicked() {
        actions.push(MenuAction::OpenFile);
        ui.close();
    }
    if !settings.recent_roms.is_empty() {
        ui.menu_button("Recent ROMs", |ui| {
            let recent = settings.recent_roms.clone();
            for entry in &recent {
                let path = std::path::Path::new(&entry.path);
                let exists = path.exists();
                let label = if exists {
                    entry.name.clone()
                } else {
                    format!("{} (missing)", entry.name)
                };
                let button = ui.add_enabled(exists, egui::Button::new(label));
                if button.on_hover_text(&entry.path).clicked() {
                    actions.push(MenuAction::LoadRecentRom(PathBuf::from(&entry.path)));
                    ui.close();
                }
            }
        });
    }
    if ui.button("Stop").clicked() {
        actions.push(MenuAction::StopGame);
        ui.close();
    }
    if ui.button("Reset Game").clicked() {
        actions.push(MenuAction::ResetGame);
        ui.close();
    }
    if ui.button("Settings").clicked() {
        actions.push(MenuAction::OpenSettings);
        ui.close();
    }
    ui.separator();
    ui.menu_button("Save State", |ui| {
        for slot in 0..=9u8 {
            if ui.button(&slot_labels[slot as usize]).clicked() {
                actions.push(MenuAction::SaveStateSlot(slot));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Save to File...").clicked() {
            actions.push(MenuAction::SaveStateFile);
            ui.close();
        }
    });
    ui.menu_button("Load State", |ui| {
        for slot in 0..=9u8 {
            if ui.button(&slot_labels[slot as usize]).clicked() {
                actions.push(MenuAction::LoadStateSlot(slot));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Load from File...").clicked() {
            actions.push(MenuAction::LoadStateFile);
            ui.close();
        }
    });
    ui.separator();
    if is_recording_audio {
        if ui.button("⏹ Stop Recording").clicked() {
            actions.push(MenuAction::StopAudioRecording);
            ui.close();
        }
    } else if ui.button("⏺ Record Audio...").clicked() {
        actions.push(MenuAction::StartAudioRecording);
        ui.close();
    }
    ui.separator();
    if is_recording_replay {
        if ui.button("⏹ Stop Replay Recording").clicked() {
            actions.push(MenuAction::StopReplayRecording);
            ui.close();
        }
    } else if is_playing_replay {
        ui.label("▶ Replay playing...");
    } else {
        if ui.button("⏺ Record Replay...").clicked() {
            actions.push(MenuAction::StartReplayRecording);
            ui.close();
        }
        if ui.button("▶ Play Replay...").clicked() {
            actions.push(MenuAction::LoadReplay);
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Screenshot...").clicked() {
        actions.push(MenuAction::TakeScreenshot);
        ui.close();
    }
}

