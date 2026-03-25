use crate::cheats::{CheatCode, CheatPatch, export_cht_file, parse_cheat, parse_cht_file};
use crate::debug::CheatState;
use crate::debug::libretro_cheats;

pub(super) fn draw_cheats_content(ui: &mut egui::Ui, state: &mut CheatState) {
    ui.heading("Cheat Codes");
    ui.label("GameShark (01VVAAAA, supports ??/?0/0?), Game Genie (XXX-YYY or XXX-YYY-ZZZ), XPloder ($XXXXXXXX), or raw (AAAA:VV)");

    ui.horizontal(|ui| {
        ui.label("Code:");
        let response = ui.text_edit_singleline(&mut state.input);
        let enter_pressed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        let add_clicked = ui.button("Add").clicked();

        if (add_clicked || enter_pressed) && !state.input.trim().is_empty() {
            match parse_cheat(&state.input) {
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

    // --- Import / Export (user cheats only) ---
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
                        let imported = parse_cht_file(&content);
                        let count = imported.len();
                        state.user_codes.extend(imported);
                        state.parse_error = None;
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

    ui.separator();

    // --- User Cheats Section ---
    draw_cheat_section(ui, "👤 User Cheats", &mut state.user_codes, None);

    ui.separator();

    // --- Libretro Database Section ---
    draw_libretro_section(ui, state);

    ui.separator();

    // --- Libretro Cheats List ---
    let mut copy_to_user: Option<CheatCode> = None;
    draw_cheat_section(
        ui,
        "🌐 Libretro Cheats",
        &mut state.libretro_codes,
        Some(&mut copy_to_user),
    );
    if let Some(cheat) = copy_to_user {
        state.user_codes.push(cheat);
    }
}

fn draw_cheat_section(
    ui: &mut egui::Ui,
    label: &str,
    codes: &mut Vec<CheatCode>,
    mut copy_target: Option<&mut Option<CheatCode>>,
) {
    if codes.is_empty() {
        ui.label(format!("{label}: none"));
        return;
    }

    let active_count = codes.iter().filter(|c| c.enabled).count();
    ui.label(format!(
        "{label}: {} cheat(s), {} active",
        codes.len(),
        active_count
    ));

    let mut remove_idx = None;
    for (i, cheat) in codes.iter_mut().enumerate() {
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
                cheat.name, cheat.code_text, summary, type_label,
            );
            ui.label(label);
            if let Some(param) = cheat.parameter_value.as_mut() {
                ui.label("Value:");
                ui.add(egui::DragValue::new(param).range(0..=255));
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
    }

    ui.horizontal(|ui| {
        if ui.button("Enable All").clicked() {
            for cheat in codes.iter_mut() {
                cheat.enabled = true;
            }
        }
        if ui.button("Disable All").clicked() {
            for cheat in codes.iter_mut() {
                cheat.enabled = false;
            }
        }
        if ui.button("Clear All").clicked() {
            codes.clear();
        }
    });
}

fn draw_libretro_section(ui: &mut egui::Ui, state: &mut CheatState) {
    let header =
        egui::CollapsingHeader::new("🌐 libretro Cheat Database").default_open(state.libretro_show);
    let response = header.show(ui, |ui| {
        ui.label(
            egui::RichText::new("Search and download cheats from the libretro-database.")
                .small()
                .color(egui::Color32::GRAY),
        );

        let platform_label = if state.rom_is_gbc {
            "Game Boy Color"
        } else {
            "Game Boy"
        };
        ui.label(format!("Platform: {platform_label}"));
        if let Some(crc32) = state.rom_crc32 {
            ui.label(format!("ROM CRC32: {crc32:08X}"));
        }
        if let Some(ref title) = state.rom_metadata_title {
            ui.label(format!("Matched metadata: {title}"));
        }
        if let Some(ref rom_name) = state.rom_metadata_rom_name {
            ui.label(format!("Metadata ROM name: {rom_name}"));
        }

        ui.horizontal(|ui| {
            ui.label("Search:");
            let search_response = ui.text_edit_singleline(&mut state.libretro_search);
            let enter_pressed =
                search_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

            if ui.button("🔍 Search").clicked() || enter_pressed {
                do_libretro_search(state);
            }
        });

        ui.horizontal(|ui| {
            if ui.selectable_label(!state.rom_is_gbc, "GB").clicked() {
                state.rom_is_gbc = false;
                state.libretro_file_list = None;
                state.libretro_results.clear();
            }
            if ui.selectable_label(state.rom_is_gbc, "GBC").clicked() {
                state.rom_is_gbc = true;
                state.libretro_file_list = None;
                state.libretro_results.clear();
            }
            ui.separator();
            if ui
                .small_button("🌐 Browse online")
                .on_hover_text("Open the libretro cheat database in your browser")
                .clicked()
            {
                let url = libretro_cheats::browse_url(state.rom_is_gbc);
                let _ = open::that(url);
            }
            if ui
                .small_button("⬇ Refresh metadata")
                .on_hover_text(
                    "Download/compile local GB+GBC metadata cache from libretro dat files",
                )
                .clicked()
            {
                match crate::libretro_metadata::refresh_cache_from_libretro() {
                    Ok(stats) => {
                        if let (Some(crc32), Some(rom_title)) =
                            (state.rom_crc32, state.rom_title.as_deref())
                        {
                            let refreshed_meta =
                                crate::libretro_metadata::lookup_cached(crc32, state.rom_is_gbc);
                            state.rom_metadata_title =
                                refreshed_meta.as_ref().map(|m| m.title.clone());
                            state.rom_metadata_rom_name =
                                refreshed_meta.as_ref().map(|m| m.rom_name.clone());
                            state.libretro_search_hints =
                                crate::libretro_metadata::build_cheat_search_hints(
                                    rom_title,
                                    refreshed_meta.as_ref(),
                                );
                        }
                        state.libretro_status = Some(format!(
                            "Metadata refreshed: {} total (GB {}, GBC {})",
                            stats.total_entries, stats.gb_entries, stats.gbc_entries
                        ));
                    }
                    Err(e) => {
                        state.libretro_status = Some(format!("Failed to refresh metadata: {e}"));
                    }
                }
            }
            if ui
                .small_button("✨ Use best guess")
                .on_hover_text("Apply best metadata-derived search hint")
                .clicked()
                && let Some(best) = state.libretro_search_hints.first() {
                    state.libretro_search = best.clone();
                    do_libretro_search(state);
                }
        });

        if !state.libretro_search_hints.is_empty() {
            let preview = state
                .libretro_search_hints
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ");
            ui.label(format!("Search hints: {preview}"));
        }

        if let Some(ref status) = state.libretro_status {
            let color = if status.starts_with("Error") || status.starts_with("Failed") {
                egui::Color32::from_rgb(255, 100, 100)
            } else if status.starts_with("Imported") {
                egui::Color32::from_rgb(100, 255, 100)
            } else {
                egui::Color32::LIGHT_GRAY
            };
            ui.colored_label(color, status);
        }

        if !state.libretro_results.is_empty() {
            ui.label(format!("{} result(s):", state.libretro_results.len()));
            let results = state.libretro_results.clone();
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    let mut download_file = None;
                    for name in &results {
                        ui.horizontal(|ui| {
                            if ui
                                .small_button("⬇")
                                .on_hover_text("Download and import")
                                .clicked()
                            {
                                download_file = Some(name.clone());
                            }
                            ui.label(name);
                        });
                    }
                    if let Some(file) = download_file {
                        do_libretro_download(state, &file);
                    }
                });
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Cheats from ")
                    .small()
                    .color(egui::Color32::GRAY),
            );
            if ui
                .link(egui::RichText::new("libretro-database").small())
                .clicked()
            {
                let _ = open::that("https://github.com/libretro/libretro-database");
            }
            ui.label(
                egui::RichText::new(" · CC-BY-SA-4.0")
                    .small()
                    .color(egui::Color32::GRAY),
            );
        });
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Found new cheats? Consider ")
                    .small()
                    .color(egui::Color32::from_rgb(180, 180, 100)),
            );
            if ui
                .link(egui::RichText::new("contributing to libretro-database").small())
                .clicked()
            {
                let _ = open::that("https://github.com/libretro/libretro-database/tree/master/cht");
            }
            ui.label(
                egui::RichText::new("!")
                    .small()
                    .color(egui::Color32::from_rgb(180, 180, 100)),
            );
        });
    });

    state.libretro_show = response.openness > 0.0;
}

fn do_libretro_search(state: &mut CheatState) {
    let cache_dir = libretro_cheats::libretro_cache_dir();

    if state.libretro_search.trim().is_empty()
        && let Some(best_hint) = state.libretro_search_hints.first() {
            state.libretro_search = best_hint.clone();
        }

    if state.libretro_file_list.is_none() {
        state.libretro_status = Some("Fetching file list from GitHub...".to_string());
        match libretro_cheats::fetch_cheat_list(state.rom_is_gbc, &cache_dir) {
            Ok(list) => {
                state.libretro_status = Some(format!("Found {} cheat files", list.len()));
                state.libretro_file_list = Some(list);
            }
            Err(e) => {
                state.libretro_status = Some(format!("Error: {e}"));
                return;
            }
        }
    }

    if let Some(ref file_list) = state.libretro_file_list {
        let results = libretro_cheats::search_filenames_with_hints(
            &state.libretro_search,
            file_list,
            50,
            &state.libretro_search_hints,
        );
        let count = results.len();
        state.libretro_results = results;
        state.libretro_status = Some(format!("{count} match(es) found"));
    }
}

fn do_libretro_download(state: &mut CheatState, filename: &str) {
    let cache_dir = libretro_cheats::libretro_cache_dir();

    match libretro_cheats::download_cht_content(filename, state.rom_is_gbc, &cache_dir) {
        Ok(content) => {
            let imported = parse_cht_file(&content);
            let count = imported.len();
            state.libretro_codes.extend(imported);
            state.libretro_status = Some(format!("Imported {count} cheat(s) from {filename}"));
            log::info!("Imported {} cheats from libretro: {}", count, filename);
        }
        Err(e) => {
            state.libretro_status = Some(format!("Failed to download: {e}"));
            log::warn!("Failed to download cheat file {}: {}", filename, e);
        }
    }
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
