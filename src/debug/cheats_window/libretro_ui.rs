use crate::cheats::parse_cht_file;
use crate::debug::CheatState;
use crate::debug::libretro_cheats;
use crate::emu_backend::ActiveSystem;

pub(super) fn draw_libretro_section(ui: &mut egui::Ui, state: &mut CheatState) {
    let header =
        egui::CollapsingHeader::new("🌐 libretro Cheat Database").default_open(state.libretro_show);
    let response = header.show(ui, |ui| {
        ui.label(
            egui::RichText::new("Search and download cheats from the libretro-database.")
                .small()
                .color(egui::Color32::GRAY),
        );

        let platform_label = match state.active_system {
            ActiveSystem::Nes => "NES",
            ActiveSystem::GameBoy if state.rom_is_gbc => "Game Boy Color",
            ActiveSystem::GameBoy => "Game Boy",
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

        draw_platform_and_actions(ui, state);

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

        draw_search_results(ui, state);

        draw_attribution(ui);
    });

    state.libretro_show = response.openness > 0.0;
}

fn draw_platform_and_actions(ui: &mut egui::Ui, state: &mut CheatState) {
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
            if let Err(e) = open::that(url) {
                log::warn!("failed to open browser: {e}");
            }
        }
        if ui
            .small_button("⬇ Refresh metadata")
            .on_hover_text("Download/compile local GB+GBC metadata cache from libretro dat files")
            .clicked()
        {
            match crate::libretro_metadata::refresh_cache_from_libretro() {
                Ok(stats) => {
                    if let (Some(crc32), Some(rom_title)) =
                        (state.rom_crc32, state.rom_title.as_deref())
                    {
                        let refreshed_meta =
                            crate::libretro_metadata::lookup_cached(crc32, state.rom_is_gbc);
                        state.rom_metadata_title = refreshed_meta.as_ref().map(|m| m.title.clone());
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
            && let Some(best) = state.libretro_search_hints.first()
        {
            state.libretro_search = best.clone();
            do_libretro_search(state);
        }
    });
}

fn draw_search_results(ui: &mut egui::Ui, state: &mut CheatState) {
    if state.libretro_results.is_empty() {
        return;
    }

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

fn draw_attribution(ui: &mut egui::Ui) {
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
            && let Err(e) = open::that("https://github.com/libretro/libretro-database")
        {
            log::warn!("failed to open browser: {e}");
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
            && let Err(e) =
                open::that("https://github.com/libretro/libretro-database/tree/master/cht")
        {
            log::warn!("failed to open browser: {e}");
        }
        ui.label(
            egui::RichText::new("!")
                .small()
                .color(egui::Color32::from_rgb(180, 180, 100)),
        );
    });
}

fn do_libretro_search(state: &mut CheatState) {
    let cache_dir = libretro_cheats::libretro_cache_dir();

    if state.libretro_search.trim().is_empty()
        && let Some(best_hint) = state.libretro_search_hints.first()
    {
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
            state.cheats_dirty = true;
            state.libretro_status = Some(format!("Imported {count} cheat(s) from {filename}"));
            log::info!("Imported {} cheats from libretro: {}", count, filename);
        }
        Err(e) => {
            state.libretro_status = Some(format!("Failed to download: {e}"));
            log::warn!("Failed to download cheat file {}: {}", filename, e);
        }
    }
}
