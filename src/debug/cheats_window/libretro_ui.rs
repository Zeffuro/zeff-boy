use crate::cheats::parse_cht_file_for_system;
use crate::debug::libretro_cheats;
use crate::debug::{CheatState, LibretroAsyncResult};
use crate::emu_backend::ActiveSystem;
use crate::libretro_common::LibretroPlatform;

pub(super) fn draw_libretro_section(ui: &mut egui::Ui, state: &mut CheatState) {
    poll_async_results(state);

    let header =
        egui::CollapsingHeader::new("🌐 libretro Cheat Database").default_open(state.libretro_show);
    let response = header.show(ui, |ui| {
        ui.label(
            egui::RichText::new("Search and download cheats from the libretro-database.")
                .small()
                .color(egui::Color32::GRAY),
        );

        ui.label(format!("Platform: {}", state.libretro_platform.label()));
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

            let can_search = !state.libretro_busy;
            if (ui
                .add_enabled(can_search, egui::Button::new("🔍 Search"))
                .clicked()
                || enter_pressed)
                && can_search
            {
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
            } else if state.libretro_busy {
                egui::Color32::from_rgb(200, 200, 100)
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

#[cfg(not(target_arch = "wasm32"))]
fn poll_async_results(state: &mut CheatState) {
    let Some(rx) = state.libretro_rx.take() else {
        return;
    };
    let mut has_pending = false;
    while let Ok(result) = rx.try_recv() {
        state.libretro_busy = false;
        match result {
            LibretroAsyncResult::FileList(Ok(list)) => {
                state.libretro_status = Some(format!("Found {} cheat files", list.len()));
                state.libretro_file_list = Some(list);
                run_local_search(state);
            }
            LibretroAsyncResult::FileList(Err(e)) => {
                state.libretro_status = Some(format!("Error: {e}"));
            }
            LibretroAsyncResult::Downloaded {
                filename,
                result: Ok(content),
            } => {
                let imported = parse_cht_file_for_system(&content, state.active_system);
                let count = imported.len();
                state.libretro_codes.extend(imported);
                state.cheats_dirty = true;
                state.libretro_status = Some(format!("Imported {count} cheat(s) from {filename}"));
                log::info!("Imported {} cheats from libretro: {}", count, filename);
            }
            LibretroAsyncResult::Downloaded {
                filename,
                result: Err(e),
            } => {
                state.libretro_status = Some(format!("Failed to download: {e}"));
                log::warn!("Failed to download cheat file {}: {}", filename, e);
            }
            LibretroAsyncResult::MetadataRefreshed(Ok(stats)) => {
                let rom_crc32 = state.rom_crc32;
                let rom_title = state.rom_title.clone();
                let platform = state.libretro_platform;
                if let (Some(crc32), Some(title)) = (rom_crc32, rom_title) {
                    let refreshed_meta = crate::libretro_metadata::lookup_cached(crc32, platform);
                    state.rom_metadata_title = refreshed_meta.as_ref().map(|m| m.title.clone());
                    state.rom_metadata_rom_name =
                        refreshed_meta.as_ref().map(|m| m.rom_name.clone());
                    state.libretro_search_hints =
                        crate::libretro_metadata::build_cheat_search_hints(
                            &title,
                            refreshed_meta.as_ref(),
                        );
                }
                state.libretro_status = Some(format!(
                    "Metadata refreshed: {} total (GB {}, GBC {}, NES {})",
                    stats.total_entries, stats.gb_entries, stats.gbc_entries, stats.nes_entries
                ));
            }
            LibretroAsyncResult::MetadataRefreshed(Err(e)) => {
                state.libretro_status = Some(format!("Failed to refresh metadata: {e}"));
            }
        }
    }
    if state.libretro_busy {
        has_pending = true;
    }
    if has_pending {
        state.libretro_rx = Some(rx);
    }
}

#[cfg(target_arch = "wasm32")]
fn poll_async_results(_state: &mut CheatState) {}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_async(state: &mut CheatState) -> crossbeam_channel::Sender<LibretroAsyncResult> {
    let (tx, rx) = crossbeam_channel::unbounded();
    state.libretro_rx = Some(rx);
    state.libretro_busy = true;
    tx
}

fn draw_platform_and_actions(ui: &mut egui::Ui, state: &mut CheatState) {
    ui.horizontal(|ui| {
        let switch_platform = |state: &mut CheatState, new: LibretroPlatform| {
            if state.libretro_platform != new {
                state.libretro_platform = new;
                state.libretro_file_list = None;
                state.libretro_results.clear();
            }
        };

        match state.active_system {
            ActiveSystem::GameBoy => {
                if ui
                    .selectable_label(state.libretro_platform == LibretroPlatform::Gb, "GB")
                    .clicked()
                {
                    switch_platform(state, LibretroPlatform::Gb);
                }
                if ui
                    .selectable_label(state.libretro_platform == LibretroPlatform::Gbc, "GBC")
                    .clicked()
                {
                    switch_platform(state, LibretroPlatform::Gbc);
                }
            }
            ActiveSystem::Nes => {
                ui.label("NES");
            }
        }
        ui.separator();
        if ui
            .small_button("🌐 Browse online")
            .on_hover_text("Open the libretro cheat database in your browser")
            .clicked()
        {
            let url = libretro_cheats::browse_url(state.libretro_platform);
            #[cfg(not(target_arch = "wasm32"))]
            if let Err(e) = open::that(url) {
                log::warn!("failed to open browser: {e}");
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = web_sys::window().map(|w| { let _ = w.open_with_url(&url); });
            }
        }
        let can_refresh = !state.libretro_busy;
        if ui
            .add_enabled(can_refresh, egui::Button::new("⬇ Refresh metadata").small())
            .on_hover_text(
                "Download/compile local metadata cache from libretro dat files (GB+GBC+NES)",
            )
            .clicked()
        {
            #[cfg(not(target_arch = "wasm32"))]
            {
                state.libretro_status = Some("Refreshing metadata...".to_string());
                let tx = spawn_async(state);
                std::thread::spawn(move || {
                    let result = crate::libretro_metadata::refresh_cache_from_libretro();
                    let _ = tx.send(LibretroAsyncResult::MetadataRefreshed(result));
                });
            }
            #[cfg(target_arch = "wasm32")]
            {
                state.libretro_status = Some("Not available on web".to_string());
            }
        }
        let can_guess = !state.libretro_busy;
        if ui
            .add_enabled(can_guess, egui::Button::new("✨ Use best guess").small())
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
    let can_download = !state.libretro_busy;
    egui::ScrollArea::vertical()
        .max_height(200.0)
        .show(ui, |ui| {
            let mut download_file = None;
            for name in &results {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_download, egui::Button::new("⬇").small())
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
        {
            #[cfg(not(target_arch = "wasm32"))]
            if let Err(e) = open::that("https://github.com/libretro/libretro-database") {
                log::warn!("failed to open browser: {e}");
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = web_sys::window().and_then(|w| w.open_with_url("https://github.com/libretro/libretro-database").ok());
            }
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
            #[cfg(not(target_arch = "wasm32"))]
            if let Err(e) = open::that("https://github.com/libretro/libretro-database/tree/master/cht") {
                log::warn!("failed to open browser: {e}");
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = web_sys::window().and_then(|w| w.open_with_url("https://github.com/libretro/libretro-database/tree/master/cht").ok());
            }
        }
        ui.label(
            egui::RichText::new("!")
                .small()
                .color(egui::Color32::from_rgb(180, 180, 100)),
        );
    });
}

fn do_libretro_search(state: &mut CheatState) {
    if state.libretro_search.trim().is_empty()
        && let Some(best_hint) = state.libretro_search_hints.first()
    {
        state.libretro_search = best_hint.clone();
    }

    if state.libretro_file_list.is_some() {
        run_local_search(state);
        return;
    }

    if state.libretro_busy {
        return;
    }

    state.libretro_status = Some("Fetching file list from GitHub...".to_string());
    let platform = state.libretro_platform;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let tx = spawn_async(state);
        std::thread::spawn(move || {
            let cache_dir = libretro_cheats::libretro_cache_dir();
            let result = libretro_cheats::fetch_cheat_list(platform, &cache_dir);
            let _ = tx.send(LibretroAsyncResult::FileList(result));
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        state.libretro_status = Some("Not available on web".to_string());
    }
}

fn run_local_search(state: &mut CheatState) {
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
    if state.libretro_busy {
        return;
    }

    state.libretro_status = Some(format!("Downloading {}...", filename));
    let platform = state.libretro_platform;
    let filename_owned = filename.to_string();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let tx = spawn_async(state);
        std::thread::spawn(move || {
            let cache_dir = libretro_cheats::libretro_cache_dir();
            let result = libretro_cheats::download_cht_content(&filename_owned, platform, &cache_dir);
            let _ = tx.send(LibretroAsyncResult::Downloaded {
                filename: filename_owned,
                result,
            });
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        state.libretro_status = Some("Not available on web".to_string());
    }
}
