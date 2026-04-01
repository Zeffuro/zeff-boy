mod cheat_list;
mod libretro_ui;

use crate::cheats::{
    CheatCode, CheatPatch, export_cht_file, parse_cheat_for_system, parse_cht_file_for_system,
};
use crate::debug::CheatState;

pub(super) fn draw_cheats_content(ui: &mut egui::Ui, state: &mut CheatState) {
    ui.heading("Cheat Codes");
    let help_text = match state.active_system {
        crate::emu_backend::ActiveSystem::Nes => {
            "NES Game Genie (AAAAAA or AAAAAAAA), GameShark (01VVAAAA), or raw (AAAA:VV)"
        }
        crate::emu_backend::ActiveSystem::GameBoy => {
            "GameShark (01VVAAAA, supports ??/?0/0?), Game Genie (XXX-YYY or XXX-YYY-ZZZ), XPloder ($XXXXXXXX), or raw (AAAA:VV)"
        }
    };
    ui.label(help_text);

    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label("Code:");
        let response = ui.text_edit_singleline(&mut state.input);
        let enter_pressed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        let add_clicked = ui.button("Add").clicked();

        if (add_clicked || enter_pressed) && !state.input.trim().is_empty() {
            match parse_cheat_for_system(&state.input, state.active_system) {
                Ok((patches, code_type)) => {
                    let name = if state.name_input.trim().is_empty() {
                        patches_summary(&patches)
                    } else {
                        state.name_input.trim().to_string()
                    };
                    let parameter_value =
                        patches.iter().copied().find_map(|p| p.default_user_value());
                    state.user_codes.push(CheatCode {
                        name,
                        code_text: state.input.trim().to_string(),
                        enabled: true,
                        parameter_value,
                        code_type,
                        patches,
                    });
                    state.input.clear();
                    state.name_input.clear();
                    state.parse_error = None;
                    changed = true;
                }
                Err(msg) => {
                    state.parse_error = Some(msg.to_string());
                }
            }
        }
    });

    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut state.name_input);
    });

    if let Some(ref err) = state.parse_error {
        ui.colored_label(egui::Color32::RED, err);
    }

    ui.separator();

    draw_import_export(ui, state, &mut changed);

    ui.separator();

    changed |= cheat_list::draw_cheat_section(ui, "👤 User Cheats", &mut state.user_codes, None);

    ui.separator();

    libretro_ui::draw_libretro_section(ui, state);

    ui.separator();

    let mut copy_to_user: Option<CheatCode> = None;
    changed |= cheat_list::draw_cheat_section(
        ui,
        "🌐 Libretro Cheats",
        &mut state.libretro_codes,
        Some(&mut copy_to_user),
    );
    if let Some(cheat) = copy_to_user {
        state.user_codes.push(cheat);
        changed = true;
    }

    if changed {
        state.cheats_dirty = true;
    }
}

fn draw_import_export(ui: &mut egui::Ui, state: &mut CheatState, changed: &mut bool) {
    ui.horizontal(|ui| {
        if ui
            .button("📂 Import .cht")
            .on_hover_text("Import cheats from a .cht file into user cheats")
            .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Cheat files", &["cht", "txt"])
                .pick_file()
        {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let imported = parse_cht_file_for_system(&content, state.active_system);
                    let count = imported.len();
                    state.user_codes.extend(imported);
                    state.parse_error = None;
                    *changed = true;
                    log::info!("Imported {} cheats from {}", count, path.display());
                }
                Err(e) => {
                    state.parse_error = Some(format!("Failed to read file: {e}"));
                }
            }
        }
        if !state.user_codes.is_empty()
            && ui
                .button("💾 Export .cht")
                .on_hover_text("Export user cheats to a .cht file")
                .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Cheat files", &["cht"])
                .set_file_name("cheats.cht")
                .save_file()
        {
            let content = export_cht_file(&state.user_codes);
            match std::fs::write(&path, content) {
                Ok(()) => {
                    log::info!(
                        "Exported {} cheats to {}",
                        state.user_codes.len(),
                        path.display()
                    );
                }
                Err(e) => {
                    state.parse_error = Some(format!("Failed to write file: {e}"));
                }
            }
        }
    });
}

pub(super) fn patches_summary(patches: &[CheatPatch]) -> String {
    patches
        .iter()
        .map(|p| match *p {
            CheatPatch::RamWrite { address, value } => {
                format!("{address:04X}={}", value.display())
            }
            CheatPatch::RomWrite { address, value } => {
                format!("ROM {address:04X}={}", value.display())
            }
            CheatPatch::RomWriteIfEquals {
                address,
                value,
                compare,
            } => {
                format!(
                    "ROM {address:04X}={}?{}",
                    value.display(),
                    compare.display()
                )
            }
            CheatPatch::RamWriteIfEquals {
                address,
                value,
                compare,
            } => {
                format!("{address:04X}={}?{}", value.display(), compare.display())
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
