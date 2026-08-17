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
    let current = disassembly.and_then(|view| symbols.source_reference_for_disassembly(view));
    let current_source_line = current
        .as_ref()
        .map(|reference| (reference.source_file, reference.line));
    if let Some(reference) = &current {
        ui.horizontal_wrapped(|ui| {
            ui.monospace(format!("{}:{}", reference.display_path, reference.line));
            if ui.button("Open source").clicked() {
                load_source(state, reference);
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
    if symbols.exec_mode() != crate::symbols::ExecMode::Sm83 {
        return;
    }
    let offsets = symbols.source_breakpoint_offsets(source_file, line);
    if offsets.is_empty() {
        return;
    }
    let set = cpu_debug.is_some_and(|info| {
        offsets
            .iter()
            .all(|offset| info.rom_breakpoints.contains(offset))
    });
    let action = if set { "Remove" } else { "Set" };
    let label = if offsets.len() == 1 {
        format!("{action} breakpoint")
    } else {
        format!("{action} {} breakpoints", offsets.len())
    };
    if ui.small_button(label).clicked() {
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
}
