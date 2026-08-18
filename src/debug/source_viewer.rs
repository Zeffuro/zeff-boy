use crate::debug::types::SourceViewerState;
use crate::symbols::{SourceReference, SymbolSession};

const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;

pub(super) fn draw_source_viewer_content(
    ui: &mut egui::Ui,
    state: &mut SourceViewerState,
    symbols: &SymbolSession,
    disassembly: Option<&crate::debug::DisassemblyView>,
    cpu_debug: Option<&crate::debug::CpuDebugSnapshot>,
    actions: &mut crate::debug::DebugUiActions,
) {
    let current = disassembly
        .and_then(|view| symbols.source_reference_for_disassembly(view))
        .map(|mut reference| {
            if let Some(path) = state.source_path_overrides.get(&reference.source_file) {
                reference.path.clone_from(path);
            } else if let Some(path) =
                remap_source_path(&reference.path, &state.source_root_mappings)
            {
                reference.path = path;
            }
            reference
        });
    let current_source_line = current
        .as_ref()
        .map(|reference| (reference.source_file, reference.line));
    if let Some(reference) = &current {
        ui.horizontal_wrapped(|ui| {
            ui.monospace(format!("{}:{}", reference.display_path, reference.line));
            if ui.button("Open source").clicked() {
                load_source(state, reference);
            }
            #[cfg(not(target_arch = "wasm32"))]
            if ui.small_button("Locate...").clicked()
                && let Some(path) = crate::platform::FileDialog::new()
                    .set_title("Locate source file")
                    .pick_file()
            {
                state
                    .source_path_overrides
                    .insert(reference.source_file, path.clone());
                let mut mapped = reference.clone();
                mapped.path = path;
                load_source(state, &mapped);
            }
            #[cfg(not(target_arch = "wasm32"))]
            if ui.small_button("Map folder...").clicked()
                && let Some(source_root) = reference.path.parent()
                && let Some(mapped_root) = crate::platform::FileDialog::new()
                    .set_title("Select matching source folder")
                    .pick_folder()
            {
                state
                    .source_root_mappings
                    .retain(|(root, _)| root != source_root);
                state
                    .source_root_mappings
                    .push((source_root.to_path_buf(), mapped_root));
                if let Some(path) = remap_source_path(&reference.path, &state.source_root_mappings)
                {
                    let mut mapped = reference.clone();
                    mapped.path = path;
                    load_source(state, &mapped);
                }
            }
            if state.loaded_path.as_ref() == Some(&reference.path)
                && ui.button("Current line").clicked()
            {
                state.selected_line = Some(reference.line);
            }
            draw_breakpoint_button(
                ui,
                symbols,
                cpu_debug,
                actions,
                reference.source_file,
                reference.line,
            );
        });
    } else {
        ui.label("No source mapping for the current instruction.");
    }

    if let Some(status) = &state.status {
        ui.weak(status);
    }
    if state.lines.is_empty() {
        return;
    }

    ui.separator();
    let selected = state.selected_line;
    let row_height = crate::debug::common::debug_mono_font(ui).size + 4.0;
    egui::ScrollArea::both().show_rows(ui, row_height, state.lines.len(), |ui, range| {
        for index in range {
            let line_number = index as u32 + 1;
            let text = format!("{:>6}  {}", line_number, state.lines[index]);
            let mut rich = egui::RichText::new(text).monospace();
            if selected == Some(line_number) {
                rich = rich.background_color(crate::debug::common::color32(
                    crate::debug::common::debug_colors(ui).pc,
                ));
            }
            if ui
                .add(egui::Label::new(rich).sense(egui::Sense::click()))
                .clicked()
            {
                state.selected_line = Some(line_number);
            }
        }
    });
    if let (Some(source_file), Some(line)) = (state.loaded_source_file, state.selected_line)
        && current_source_line != Some((source_file, line))
    {
        ui.horizontal(|ui| {
            ui.monospace(format!("Line {line}"));
            draw_breakpoint_button(ui, symbols, cpu_debug, actions, source_file, line);
        });
    }
}

fn draw_breakpoint_button(
    ui: &mut egui::Ui,
    symbols: &SymbolSession,
    cpu_debug: Option<&crate::debug::CpuDebugSnapshot>,
    actions: &mut crate::debug::DebugUiActions,
    source_file: usize,
    line: u32,
) {
    let offsets = symbols.source_breakpoint_offsets(source_file, line);
    if symbols.exec_mode() == crate::symbols::ExecMode::Sm83 && !offsets.is_empty() {
        let set = cpu_debug.is_some_and(|info| {
            offsets
                .iter()
                .all(|offset| info.rom_breakpoints.contains(offset))
        });
        if breakpoint_button(ui, set, offsets.len()).clicked() {
            if set {
                actions.remove_rom_breakpoints.extend_from_slice(offsets);
            } else if let Some(info) = cpu_debug {
                actions.toggle_rom_breakpoints.extend(
                    offsets
                        .iter()
                        .filter(|offset| !info.rom_breakpoints.contains(offset))
                        .copied(),
                );
            }
        }
        return;
    }

    let addresses = symbols.source_breakpoint_addresses(source_file, line);
    if addresses.is_empty() {
        return;
    }
    let set = cpu_debug.is_some_and(|info| {
        addresses
            .iter()
            .all(|address| info.breakpoints.contains(address))
    });
    if breakpoint_button(ui, set, addresses.len()).clicked() {
        if set {
            actions.remove_breakpoints.extend_from_slice(addresses);
        } else if let Some(info) = cpu_debug {
            actions.toggle_breakpoints.extend(
                addresses
                    .iter()
                    .filter(|address| !info.breakpoints.contains(address))
                    .copied(),
            );
        }
    }
}

fn breakpoint_button(ui: &mut egui::Ui, set: bool, count: usize) -> egui::Response {
    let action = if set { "Remove" } else { "Set" };
    let label = if count == 1 {
        format!("{action} breakpoint")
    } else {
        format!("{action} {count} breakpoints")
    };
    ui.small_button(label)
}

fn remap_source_path(
    path: &std::path::Path,
    mappings: &[(std::path::PathBuf, std::path::PathBuf)],
) -> Option<std::path::PathBuf> {
    mappings
        .iter()
        .filter_map(|(source, target)| {
            path.strip_prefix(source)
                .ok()
                .map(|rest| (source, target, rest))
        })
        .max_by_key(|(source, _, _)| source.components().count())
        .map(|(_, target, rest)| target.join(rest))
}

fn load_source(state: &mut SourceViewerState, reference: &SourceReference) {
    let result = (|| -> anyhow::Result<(Vec<String>, Option<String>)> {
        let metadata = std::fs::metadata(&reference.path)?;
        anyhow::ensure!(
            metadata.len() <= MAX_SOURCE_BYTES,
            "source file is larger than 4 MiB"
        );
        let bytes = std::fs::read(&reference.path)?;
        let status = reference.crc32.and_then(|expected| {
            let actual = crc32fast::hash(&bytes);
            (actual != expected).then(|| format!("CRC differs from symbol file ({actual:08X})"))
        });
        let text = String::from_utf8_lossy(&bytes);
        Ok((text.lines().map(str::to_owned).collect(), status))
    })();
    state.selected_line = Some(reference.line);
    state.loaded_source_file = Some(reference.source_file);
    state.loaded_path = Some(reference.path.clone());
    match result {
        Ok((lines, status)) => {
            state.lines = lines;
            state.status = status.or_else(|| Some(reference.path.display().to_string()));
        }
        Err(error) => {
            state.lines.clear();
            state.status = Some(format!("{}: {error}", reference.path.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_limit_is_reasonable_for_debug_panes() {
        assert_eq!(MAX_SOURCE_BYTES, 4 * 1024 * 1024);
    }

    #[test]
    fn source_mapping_prefers_the_longest_root() {
        let mappings = vec![
            ("/build".into(), "D:/src".into()),
            ("/build/game".into(), "E:/game".into()),
        ];
        assert_eq!(
            remap_source_path(std::path::Path::new("/build/game/src/main.c"), &mappings),
            Some(std::path::PathBuf::from("E:/game/src/main.c"))
        );
    }
}
