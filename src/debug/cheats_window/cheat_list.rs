use crate::cheats::CheatCode;

pub(super) fn draw_cheat_section(
    ui: &mut egui::Ui,
    label: &str,
    codes: &mut Vec<CheatCode>,
    mut copy_target: Option<&mut Option<CheatCode>>,
) -> bool {
    if codes.is_empty() {
        ui.label(format!("{label}: none"));
        return false;
    }

    let mut changed = false;

    let active_count = codes.iter().filter(|c| c.enabled).count();
    ui.label(format!(
        "{label}: {} cheat(s), {} active",
        codes.len(),
        active_count
    ));

    let mut remove_idx = None;
    for (i, cheat) in codes.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            if ui.checkbox(&mut cheat.enabled, "").changed() {
                changed = true;
            }
            let type_label = match cheat.code_type {
                crate::cheats::CheatType::GameShark => "GS",
                crate::cheats::CheatType::GameGenie => "GG",
                crate::cheats::CheatType::XPloder => "XP",
                crate::cheats::CheatType::Raw => "Raw",
            };
            let summary = super::patches_summary(&cheat.patches);
            let label = format!(
                "{} [{}] {} ({})",
                cheat.name, cheat.code_text, summary, type_label,
            );
            ui.label(label);
            if let Some(param) = cheat.parameter_value.as_mut() {
                ui.label("Value:");
                if ui.add(egui::DragValue::new(param).range(0..=255)).changed() {
                    changed = true;
                }
                ui.label(format!("0x{param:02X}"));
            }
            if let Some(ref mut target) = copy_target
                && ui
                    .small_button("📋")
                    .on_hover_text("Copy to User Cheats")
                    .clicked()
                {
                    **target = Some(cheat.clone());
                }
            if ui.small_button("🗑").on_hover_text("Remove").clicked() {
                remove_idx = Some(i);
            }
        });
    }

    if let Some(idx) = remove_idx {
        codes.remove(idx);
        changed = true;
    }

    ui.horizontal(|ui| {
        if ui.button("Enable All").clicked() {
            for cheat in codes.iter_mut() {
                cheat.enabled = true;
            }
            changed = true;
        }
        if ui.button("Disable All").clicked() {
            for cheat in codes.iter_mut() {
                cheat.enabled = false;
            }
            changed = true;
        }
        if ui.button("Clear All").clicked() {
            codes.clear();
            changed = true;
        }
    });

    changed
}

