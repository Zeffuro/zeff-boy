use crate::debug::types::{
    MemoryViewerState, RomViewerState, SymbolBrowserState, SymbolEditorState, SymbolLocationFilter,
};
use crate::debug::{CpuDebugSnapshot, DebugTab, DebugUiActions, DisassemblyTarget};
use crate::symbols::{
    ExecMode, ProvenanceKind, SymbolKind, SymbolRecord, SymbolSession, UserSymbolDraft,
};

const MAX_RESULTS: usize = 1000;
const SYMBOL_KINDS: [SymbolKind; 6] = [
    SymbolKind::Function,
    SymbolKind::Label,
    SymbolKind::Data,
    SymbolKind::Constant,
    SymbolKind::Section,
    SymbolKind::Unknown,
];
const LOCATION_FILTERS: [SymbolLocationFilter; 4] = [
    SymbolLocationFilter::All,
    SymbolLocationFilter::Rom,
    SymbolLocationFilter::CpuOnly,
    SymbolLocationFilter::Constants,
];

pub(super) struct SymbolBrowserViews<'a> {
    pub(super) state: &'a mut SymbolBrowserState,
    pub(super) memory: &'a mut MemoryViewerState,
    pub(super) rom: &'a mut RomViewerState,
}

struct SymbolRowContext<'a> {
    exec_mode: ExecMode,
    cpu_debug: Option<&'a CpuDebugSnapshot>,
    memory: &'a mut MemoryViewerState,
    rom: &'a mut RomViewerState,
    actions: &'a mut DebugUiActions,
}

pub(super) fn draw_symbol_browser_content(
    ui: &mut egui::Ui,
    symbols: &SymbolSession,
    cpu_debug: Option<&CpuDebugSnapshot>,
    views: SymbolBrowserViews<'_>,
    actions: &mut DebugUiActions,
) {
    let SymbolBrowserViews { state, memory, rom } = views;
    if symbols.store.is_empty() {
        ui.label(if symbols.is_loading() {
            "Loading symbols..."
        } else {
            "No symbols loaded"
        });
        for diagnostic in &symbols.diagnostics {
            ui.colored_label(egui::Color32::YELLOW, diagnostic);
        }
        return;
    }

    draw_module_summary(ui, symbols);
    ui.horizontal_wrapped(|ui| {
        ui.label("Search:");
        ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .desired_width(220.0)
                .hint_text("name"),
        );
        if !state.query.is_empty() && ui.small_button("Clear").clicked() {
            state.query.clear();
        }
    });
    draw_filters(ui, state);

    if state.last_query != state.query
        || state.last_generation != symbols.store.generation()
        || state.last_kind_filter != state.kind_filter
        || state.last_location_filter != state.location_filter
    {
        let kind_filter = state.kind_filter;
        let location_filter = state.location_filter;
        state.results = symbols
            .store
            .search_ids_matching(&state.query, MAX_RESULTS, |symbol| {
                kind_filter.is_none_or(|kind| symbol.kind == kind)
                    && location_matches(symbol, location_filter)
            });
        state.last_query.clone_from(&state.query);
        state.last_generation = symbols.store.generation();
        state.last_kind_filter = state.kind_filter;
        state.last_location_filter = state.location_filter;
        if state
            .selected
            .is_some_and(|id| !state.results.contains(&id))
        {
            state.selected = None;
        }
    }

    ui.label(format!(
        "{} shown / {} loaded",
        state.results.len(),
        symbols.symbol_count()
    ));
    if let Some(symbol) = state.selected.and_then(|id| symbols.store.symbol(id)) {
        draw_symbol_details(ui, state, symbol, actions);
    }
    ui.separator();

    let mut selected = state.selected;
    let mut row = SymbolRowContext {
        exec_mode: symbols.exec_mode(),
        cpu_debug,
        memory,
        rom,
        actions,
    };
    for id in state.results.iter().copied() {
        let Some(symbol) = symbols.store.symbol(id) else {
            continue;
        };
        if draw_symbol_row(ui, symbol, selected == Some(id), &mut row) {
            selected = Some(id);
        }
    }
    state.selected = selected;
}

fn draw_module_summary(ui: &mut egui::Ui, symbols: &SymbolSession) {
    let Some(module) = symbols
        .modules
        .iter()
        .find(|module| !module.is_builtin())
        .or_else(|| symbols.modules.first())
    else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(
            module
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("symbols"),
        );
        ui.weak(format!(
            "{} | {} symbols",
            module.format, module.symbol_count
        ));
        if symbols.modules.len() > 1 {
            ui.weak(format!("{} sources", symbols.modules.len()));
        }
    });
    if symbols.modules.len() > 1 {
        egui::CollapsingHeader::new(format!("Sources ({})", symbols.modules.len())).show(
            ui,
            |ui| {
                for module in &symbols.modules {
                    ui.weak(format!(
                        "{} | {} | {} symbols",
                        module.path.display(),
                        module.format,
                        module.symbol_count
                    ));
                }
            },
        );
    }
    if symbols
        .modules
        .iter()
        .any(|module| !module.diagnostics.is_empty())
        || !symbols.diagnostics.is_empty()
    {
        egui::CollapsingHeader::new("Import warnings").show(ui, |ui| {
            for diagnostic in symbols
                .modules
                .iter()
                .flat_map(|module| module.diagnostics.iter())
                .chain(&symbols.diagnostics)
            {
                ui.colored_label(egui::Color32::YELLOW, diagnostic);
            }
        });
    }
}

fn draw_filters(ui: &mut egui::Ui, state: &mut SymbolBrowserState) {
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("symbol_kind_filter")
            .selected_text(
                state
                    .kind_filter
                    .map_or("All kinds".to_owned(), |kind| format!("{kind:?}")),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.kind_filter, None, "All kinds");
                for kind in SYMBOL_KINDS {
                    ui.selectable_value(&mut state.kind_filter, Some(kind), format!("{kind:?}"));
                }
            });
        egui::ComboBox::from_id_salt("symbol_location_filter")
            .selected_text(state.location_filter.label())
            .show_ui(ui, |ui| {
                for filter in LOCATION_FILTERS {
                    ui.selectable_value(&mut state.location_filter, filter, filter.label());
                }
            });
    });
}

fn location_matches(symbol: &SymbolRecord, filter: SymbolLocationFilter) -> bool {
    match filter {
        SymbolLocationFilter::All => true,
        SymbolLocationFilter::Rom => symbol.location.storage.is_some(),
        SymbolLocationFilter::CpuOnly => {
            symbol.location.cpu.is_some() && symbol.location.storage.is_none()
        }
        SymbolLocationFilter::Constants => symbol.value.is_some(),
    }
}

fn draw_symbol_details(
    ui: &mut egui::Ui,
    state: &mut SymbolBrowserState,
    symbol: &SymbolRecord,
    actions: &mut DebugUiActions,
) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.monospace(&symbol.name);
        egui::Grid::new("selected_symbol_details")
            .num_columns(2)
            .show(ui, |ui| {
                detail_row(ui, "Kind", format!("{:?}", symbol.kind));
                detail_row(ui, "Scope", format!("{:?}", symbol.scope));
                if let Some(bank) = symbol.location.bank {
                    detail_row(ui, "Bank", format!("{bank:X}"));
                }
                if let Some(cpu) = symbol.location.cpu {
                    detail_row(ui, "CPU", format!("{:X}", cpu.address));
                }
                if let Some(storage) = symbol.location.storage {
                    detail_row(ui, "ROM", format!("{:X}", storage.offset));
                }
                if let Some(value) = symbol.value {
                    detail_row(ui, "Value", format!("{value:X}"));
                }
                if let Some(size) = symbol.size {
                    detail_row(ui, "Size", format!("{size:X}"));
                }
                detail_row(ui, "Source", format!("{:?}", symbol.provenance.kind));
                if let Some(comment) = &symbol.comment {
                    detail_row(ui, "Comment", comment.clone());
                }
            });
        if ui.small_button("Label").clicked() {
            state.editor = Some(SymbolEditorState {
                original_user_name: (symbol.provenance.kind == ProvenanceKind::User)
                    .then(|| symbol.name.clone()),
                draft: UserSymbolDraft {
                    name: symbol.name.clone(),
                    location: symbol.location,
                    value: symbol.value,
                    kind: symbol.kind,
                    size: symbol.size,
                    comment: symbol.comment.clone(),
                },
            });
        }
        draw_editor(ui, state, actions);
    });
}

fn draw_editor(ui: &mut egui::Ui, state: &mut SymbolBrowserState, actions: &mut DebugUiActions) {
    let Some(editor) = &mut state.editor else {
        return;
    };
    let mut save = false;
    let mut remove = false;
    let mut cancel = false;
    ui.separator();
    ui.label("User label");
    ui.horizontal_wrapped(|ui| {
        ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut editor.draft.name).desired_width(180.0));
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Comment");
        ui.add(
            egui::TextEdit::singleline(editor.draft.comment.get_or_insert_with(String::new))
                .desired_width(260.0),
        );
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button("Save").clicked() {
            save = true;
        }
        if editor.original_user_name.is_some() && ui.button("Remove").clicked() {
            remove = true;
        }
        if ui.button("Cancel").clicked() {
            cancel = true;
        }
    });
    if save {
        if let Some(name) = &editor.original_user_name
            && !name.eq_ignore_ascii_case(&editor.draft.name)
        {
            actions.remove_user_symbols.push(name.clone());
        }
        actions.user_symbol = Some(editor.draft.clone());
    } else if remove && let Some(name) = &editor.original_user_name {
        actions.remove_user_symbols.push(name.clone());
    }
    if save || remove || cancel {
        state.editor = None;
        state.selected = None;
    }
}

fn detail_row(ui: &mut egui::Ui, label: &str, value: String) {
    ui.weak(label);
    ui.monospace(value);
    ui.end_row();
}

fn draw_symbol_row(
    ui: &mut egui::Ui,
    symbol: &SymbolRecord,
    selected: bool,
    row: &mut SymbolRowContext<'_>,
) -> bool {
    let mut clicked = false;
    ui.horizontal_wrapped(|ui| {
        clicked = ui.selectable_label(selected, &symbol.name).clicked();
        ui.weak(format!("{:?}", symbol.kind));
        if let Some(bank) = symbol.location.bank {
            ui.monospace(format!("B{bank:X}"));
        }
        if let Some(cpu) = symbol.location.cpu {
            ui.monospace(format!("CPU {:X}", cpu.address));
            if (symbol.location.exec_mode == ExecMode::Sm83 && cpu.address < 0x8000
                || row.exec_mode == ExecMode::Arm
                    && (0x0800_0000..=0x0DFF_FFFF).contains(&cpu.address))
                && let Some(storage) = symbol.location.storage
                && let Ok(cpu_address) = u32::try_from(cpu.address)
                && ui.small_button("Code").clicked()
            {
                row.actions.disasm_target = Some(DisassemblyTarget {
                    cpu_address,
                    storage_offset: storage.offset,
                });
                row.actions.focus_tab = Some(DebugTab::Disassembler);
            }
            if let Some(storage) = symbol.location.storage {
                let is_set = row
                    .cpu_debug
                    .is_some_and(|info| info.rom_breakpoints.contains(&storage.offset));
                let label = if is_set { "Unbreak" } else { "Break" };
                if ui.small_button(label).clicked() {
                    row.actions.toggle_rom_breakpoints.push(storage.offset);
                }
            }
            if ui.small_button("Memory").clicked()
                && let Ok(address) = u32::try_from(cpu.address)
            {
                row.memory.view_start = row.memory.address_space.clamp_start(address);
                row.memory.jump_input = row.memory.address_space.format(row.memory.view_start);
                row.actions.focus_tab = Some(DebugTab::MemoryViewer);
            }
        }
        if let Some(storage) = symbol.location.storage {
            ui.monospace(format!("ROM {:X}", storage.offset));
            if ui.small_button("ROM").clicked()
                && let Ok(offset) = u32::try_from(storage.offset)
            {
                row.rom.view_start = offset & !0xF;
                row.rom.jump_input = format!("{:06X}", row.rom.view_start);
                row.actions.focus_tab = Some(DebugTab::RomViewer);
            }
        }
    });
    clicked
}
