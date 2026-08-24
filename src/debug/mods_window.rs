use crate::debug::types::ModState;

pub(crate) fn draw_mods_content(ui: &mut egui::Ui, state: &mut ModState) {
    ui.heading("Mods (IPS / BPS / UPS / PPF / xdelta Patches)");
    ui.label(
        egui::RichText::new(
            "Drop patch files into the mods folder below. Enable or disable them here, then reload the ROM to apply.",
        )
        .small()
        .weak(),
    );

    ui.separator();

    if let Some(dir) = &state.mods_dir {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("📁 {}", dir.display()))
                    .small()
                    .weak(),
            );
        });
        ui.horizontal(|ui| {
            if ui.button("Open Folder").clicked() {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    log::warn!("Failed to create mods dir: {e}");
                }
                crate::platform::open_url(&dir.display().to_string());
            }
            if ui.button("Refresh").clicked() {
                state.entries = crate::mods::load_mod_config(dir);
                state.status_message = None;
            }
        });
    } else {
        ui.label(egui::RichText::new("No ROM loaded").weak());
        return;
    }

    ui.separator();

    if state.entries.is_empty() {
        ui.label(
            egui::RichText::new("No supported patches found in the mods folder.")
                .weak()
                .italics(),
        );
        return;
    }

    ui.label(format!(
        "{} patch{} found, {} enabled",
        state.entries.len(),
        if state.entries.len() == 1 { "" } else { "es" },
        state.enabled_count(),
    ));

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let mut changed = false;
            let mut swap: Option<(usize, usize)> = None;
            let entry_count = state.entries.len();

            for i in 0..entry_count {
                let is_xdelta = std::path::Path::new(&state.entries[i].filename)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        extension.eq_ignore_ascii_case("xdelta")
                            || extension.eq_ignore_ascii_case("xdelta3")
                            || extension.eq_ignore_ascii_case("vcdiff")
                    });
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut state.entries[i].enabled, "").changed() {
                        changed = true;
                    }
                    ui.label(&state.entries[i].filename);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_down = i + 1 < entry_count;
                        let can_up = i > 0;
                        if ui
                            .add_enabled(can_down, egui::Button::new("⬇").small())
                            .clicked()
                        {
                            swap = Some((i, i + 1));
                        }
                        if ui
                            .add_enabled(can_up, egui::Button::new("⬆").small())
                            .clicked()
                        {
                            swap = Some((i, i - 1));
                        }
                    });
                });
                if is_xdelta {
                    let mut target = state.entries[i].target.clone().unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.label("Target:");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut target)
                                .hint_text("auto from Track NN in filename"),
                        );
                        if response.changed() {
                            state.entries[i].target =
                                (!target.trim().is_empty()).then(|| target.trim().to_owned());
                            changed = true;
                        }
                    });
                }
            }

            if let Some((a, b)) = swap {
                state.entries.swap(a, b);
                changed = true;
            }

            if changed {
                state.needs_reload = true;
                if let Some(dir) = &state.mods_dir {
                    crate::mods::save_mod_config(dir, &state.entries);
                    let warnings = crate::mods::mod_advisories(dir, &state.entries);
                    state.status_message = (!warnings.is_empty()).then(|| warnings.join("\n"));
                }
            }
        });

    ui.separator();

    if state.needs_reload {
        ui.label(
            egui::RichText::new("⚠ Reload ROM to apply mod changes (File → Reset Game)")
                .color(egui::Color32::YELLOW),
        );
    }

    if let Some(msg) = &state.status_message {
        ui.label(
            egui::RichText::new(msg)
                .small()
                .color(egui::Color32::YELLOW),
        );
    }
}
