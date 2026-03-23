use crate::cheats::{CheatCode, parse_cheat};
use crate::debug::DebugWindowState;

pub(super) fn draw_cheats_content(
    ui: &mut egui::Ui,
    state: &mut DebugWindowState,
) {
    ui.heading("Cheat Codes");
    ui.label("GameShark (01VVAAAA) or raw (AAAA:VV)");

    ui.horizontal(|ui| {
        ui.label("Code:");
        let response = ui.text_edit_singleline(&mut state.cheat_input);
        let enter_pressed = response.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter));

        let add_clicked = ui.button("Add").clicked();

        if (add_clicked || enter_pressed) && !state.cheat_input.trim().is_empty() {
            match parse_cheat(&state.cheat_input) {
                Ok((address, value, code_type)) => {
                    let name = if state.cheat_name_input.trim().is_empty() {
                        format!("{:04X}={:02X}", address, value)
                    } else {
                        state.cheat_name_input.trim().to_string()
                    };
                    state.cheats.push(CheatCode {
                        name,
                        code_text: state.cheat_input.trim().to_string(),
                        address,
                        value,
                        enabled: true,
                        code_type,
                    });
                    state.cheat_input.clear();
                    state.cheat_name_input.clear();
                    state.cheat_parse_error = None;
                }
                Err(msg) => {
                    state.cheat_parse_error = Some(msg.to_string());
                }
            }
        }
    });

    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut state.cheat_name_input);
    });

    if let Some(ref err) = state.cheat_parse_error {
        ui.colored_label(egui::Color32::RED, err);
    }

    ui.separator();

    if state.cheats.is_empty() {
        ui.label("No cheats added.");
        return;
    }

    let active_count = state.cheats.iter().filter(|c| c.enabled).count();
    ui.label(format!(
        "{} cheat(s), {} active",
        state.cheats.len(),
        active_count
    ));

    ui.separator();

    let mut remove_idx = None;
    for (i, cheat) in state.cheats.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.checkbox(&mut cheat.enabled, "");
            let type_label = match cheat.code_type {
                crate::cheats::CheatType::GameShark => "GS",
                crate::cheats::CheatType::Raw => "Raw",
            };
            let label = format!(
                "{} [{}] {:04X}={:02X} ({})",
                cheat.name,
                cheat.code_text,
                cheat.address,
                cheat.value,
                type_label,
            );
            ui.label(label);
            if ui.small_button("🗑").on_hover_text("Remove").clicked() {
                remove_idx = Some(i);
            }
        });
    }

    if let Some(idx) = remove_idx {
        state.cheats.remove(idx);
    }

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Enable All").clicked() {
            for cheat in &mut state.cheats {
                cheat.enabled = true;
            }
        }
        if ui.button("Disable All").clicked() {
            for cheat in &mut state.cheats {
                cheat.enabled = false;
            }
        }
        if ui.button("Clear All").clicked() {
            state.cheats.clear();
        }
    });
}

