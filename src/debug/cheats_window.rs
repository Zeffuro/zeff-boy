use crate::cheats::{CheatCode, CheatPatch, parse_cheat};
use crate::debug::CheatState;

pub(super) fn draw_cheats_content(
    ui: &mut egui::Ui,
    state: &mut CheatState,
) {
    ui.heading("Cheat Codes");
    ui.label("GameShark (01VVAAAA, supports ??/?0/0?), Game Genie (XXX-YYY or XXX-YYY-ZZZ), XPloder ($XXXXXXXX), or raw (AAAA:VV)");

    ui.horizontal(|ui| {
        ui.label("Code:");
        let response = ui.text_edit_singleline(&mut state.input);
        let enter_pressed = response.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter));

        let add_clicked = ui.button("Add").clicked();

        if (add_clicked || enter_pressed) && !state.input.trim().is_empty() {
            match parse_cheat(&state.input) {
                Ok((patches, code_type)) => {
                    let name = if state.name_input.trim().is_empty() {
                        patches_summary(&patches)
                    } else {
                        state.name_input.trim().to_string()
                    };
                    let parameter_value = patches.iter().copied().find_map(|p| p.default_user_value());
                    state.codes.push(CheatCode {
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

    if state.codes.is_empty() {
        ui.label("No cheats added.");
        return;
    }

    let active_count = state.codes.iter().filter(|c| c.enabled).count();
    ui.label(format!(
        "{} cheat(s), {} active",
        state.codes.len(),
        active_count
    ));

    ui.separator();

    let mut remove_idx = None;
    for (i, cheat) in state.codes.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.checkbox(&mut cheat.enabled, "");
            let type_label = match cheat.code_type {
                crate::cheats::CheatType::GameShark => "GS",
                crate::cheats::CheatType::GameGenie => "GG",
                crate::cheats::CheatType::XPloder => "XP",
                crate::cheats::CheatType::Raw => "Raw",
            };
            let summary = patches_summary(&cheat.patches);
            let label = format!(
                "{} [{}] {} ({})",
                cheat.name,
                cheat.code_text,
                summary,
                type_label,
            );
            ui.label(label);
            if let Some(param) = cheat.parameter_value.as_mut() {
                ui.label("Value:");
                ui.add(egui::DragValue::new(param).range(0..=255));
                ui.label(format!("0x{param:02X}"));
            }
            if ui.small_button("🗑").on_hover_text("Remove").clicked() {
                remove_idx = Some(i);
            }
        });
    }

    if let Some(idx) = remove_idx {
        state.codes.remove(idx);
    }

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Enable All").clicked() {
            for cheat in &mut state.codes {
                cheat.enabled = true;
            }
        }
        if ui.button("Disable All").clicked() {
            for cheat in &mut state.codes {
                cheat.enabled = false;
            }
        }
        if ui.button("Clear All").clicked() {
            state.codes.clear();
        }
    });
}

fn patches_summary(patches: &[CheatPatch]) -> String {
    patches
        .iter()
        .map(|p| match *p {
            CheatPatch::RamWrite { address, value } => {
                format!("{address:04X}={}", value.display())
            }
            CheatPatch::RomWrite { address, value } => {
                format!("ROM {address:04X}={}", value.display())
            }
            CheatPatch::RomWriteIfEquals { address, value, compare } => {
                format!("ROM {address:04X}={}?{}", value.display(), compare.display())
            }
            CheatPatch::RamWriteIfEquals { address, value, compare } => {
                format!("{address:04X}={}?{}", value.display(), compare.display())
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
