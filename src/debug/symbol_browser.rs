use crate::debug::types::{
    ExecutionCoverage, MemoryViewerState, RomViewerState, RuntimeSymbolCandidate,
    SymbolBrowserState, SymbolEditorState, SymbolExecutionFilter, SymbolLocationFilter, SymbolSort,
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
const EXECUTION_FILTERS: [SymbolExecutionFilter; 3] = [
    SymbolExecutionFilter::All,
    SymbolExecutionFilter::Executed,
    SymbolExecutionFilter::Unexecuted,
];
const SYMBOL_SORTS: [SymbolSort; 2] = [SymbolSort::Search, SymbolSort::MostExecuted];

pub(super) struct SymbolBrowserViews<'a> {
    pub(super) state: &'a mut SymbolBrowserState,
    pub(super) memory: &'a mut MemoryViewerState,
    pub(super) rom: &'a mut RomViewerState,
    pub(super) coverage: &'a mut ExecutionCoverage,
}

struct SymbolRowContext<'a> {
    exec_mode: ExecMode,
    cpu_debug: Option<&'a CpuDebugSnapshot>,
    memory: &'a mut MemoryViewerState,
    rom: &'a mut RomViewerState,
    actions: &'a mut DebugUiActions,
    symbols: &'a SymbolSession,
}

pub(super) fn draw_symbol_browser_content(
    ui: &mut egui::Ui,
    symbols: &SymbolSession,
    cpu_debug: Option<&CpuDebugSnapshot>,
    views: SymbolBrowserViews<'_>,
    actions: &mut DebugUiActions,
) {
    let SymbolBrowserViews {
        state,
        memory,
        rom,
        coverage,
    } = views;
    coverage.sync_symbols(symbols);
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
        || state.last_execution_filter != state.execution_filter
        || state.last_sort != state.sort
        || state.last_coverage_revision != coverage.revision()
            && (state.execution_filter != SymbolExecutionFilter::All
                || state.sort == SymbolSort::MostExecuted)
    {
        state.results = search_symbols(symbols, coverage, state);
        state.last_query.clone_from(&state.query);
        state.last_generation = symbols.store.generation();
        state.last_kind_filter = state.kind_filter;
        state.last_location_filter = state.location_filter;
        state.last_execution_filter = state.execution_filter;
        state.last_sort = state.sort;
        state.last_coverage_revision = coverage.revision();
        if state
            .selected
            .is_some_and(|id| !state.results.contains(&id))
        {
            state.selected = None;
        }
    }

    ui.label(format!(
        "{} shown / {} loaded | {} instructions captured",
        state.results.len(),
        symbols.symbol_count(),
        coverage.total()
    ));
    draw_runtime_candidates(ui, state, coverage, actions);
    if let Some(symbol) = state.selected.and_then(|id| symbols.store.symbol(id)) {
        draw_symbol_details(ui, state, symbols, symbol, coverage.hits(symbol.id));
    }
    draw_editor(ui, state, actions);
    ui.separator();

    let mut selected = state.selected;
    let mut row = SymbolRowContext {
        exec_mode: symbols.exec_mode(),
        cpu_debug,
        memory,
        rom,
        actions,
        symbols,
    };
    for id in state.results.iter().copied() {
        let Some(symbol) = symbols.store.symbol(id) else {
            continue;
        };
        if draw_symbol_row(
            ui,
            symbol,
            coverage.hits(id),
            selected == Some(id),
            &mut row,
        ) {
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
    if !symbols.load_instances().is_empty() {
        egui::CollapsingHeader::new(format!("Runtime code ({})", symbols.load_instances().len()))
            .show(ui, |ui| {
                for instance in symbols.load_instances() {
                    let name = symbols
                        .segments()
                        .iter()
                        .find(|segment| segment.id == instance.segment)
                        .map_or("Unknown", |segment| segment.name.as_str());
                    ui.monospace(format!(
                        "{}  CPU {:X}  gen {}{}",
                        name,
                        instance.runtime_base.address,
                        instance.generation,
                        if instance.active { "" } else { "  inactive" }
                    ));
                }
            });
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
        egui::ComboBox::from_id_salt("symbol_execution_filter")
            .selected_text(state.execution_filter.label())
            .show_ui(ui, |ui| {
                for filter in EXECUTION_FILTERS {
                    if ui
                        .selectable_value(&mut state.execution_filter, filter, filter.label())
                        .clicked()
                        && filter != SymbolExecutionFilter::Executed
                    {
                        state.sort = SymbolSort::Search;
                    }
                }
            });
        egui::ComboBox::from_id_salt("symbol_sort")
            .selected_text(state.sort.label())
            .show_ui(ui, |ui| {
                for sort in SYMBOL_SORTS {
                    if ui
                        .selectable_value(&mut state.sort, sort, sort.label())
                        .clicked()
                        && sort == SymbolSort::MostExecuted
                    {
                        state.execution_filter = SymbolExecutionFilter::Executed;
                    }
                }
            });
    });
}

fn search_symbols(
    symbols: &SymbolSession,
    coverage: &ExecutionCoverage,
    state: &SymbolBrowserState,
) -> Vec<crate::symbols::SymbolId> {
    let matches = |symbol: &SymbolRecord| {
        state.kind_filter.is_none_or(|kind| symbol.kind == kind)
            && location_matches(symbol, state.location_filter)
            && match state.execution_filter {
                SymbolExecutionFilter::All => true,
                SymbolExecutionFilter::Executed => coverage.hits(symbol.id) != 0,
                SymbolExecutionFilter::Unexecuted => coverage.hits(symbol.id) == 0,
            }
    };

    if state.execution_filter == SymbolExecutionFilter::Executed {
        let query = state.query.trim().to_ascii_lowercase();
        let mut results = coverage
            .executed_ids()
            .filter_map(|id| symbols.store.symbol(id))
            .filter(|symbol| matches(symbol))
            .filter_map(|symbol| query_rank(symbol, &query).map(|rank| (symbol.id, rank)))
            .collect::<Vec<_>>();
        results.sort_unstable_by(|(left_id, left_rank), (right_id, right_rank)| {
            let hit_order = if state.sort == SymbolSort::MostExecuted {
                coverage.hits(*right_id).cmp(&coverage.hits(*left_id))
            } else {
                std::cmp::Ordering::Equal
            };
            hit_order
                .then_with(|| left_rank.cmp(right_rank))
                .then_with(|| left_id.cmp(right_id))
        });
        return results
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(id, _)| id)
            .collect();
    }

    symbols
        .store
        .search_ids_matching(&state.query, MAX_RESULTS, matches)
}

fn query_rank(symbol: &SymbolRecord, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(0);
    }
    let name = symbol.name.to_ascii_lowercase();
    if name == query {
        Some(0)
    } else if name.starts_with(query) {
        Some(1)
    } else if name.contains(query) {
        Some(2)
    } else {
        None
    }
}

fn draw_runtime_candidates(
    ui: &mut egui::Ui,
    state: &mut SymbolBrowserState,
    coverage: &ExecutionCoverage,
    actions: &mut DebugUiActions,
) {
    let mut candidates = coverage.runtime_candidates().cloned().collect::<Vec<_>>();
    if candidates.is_empty() {
        return;
    }
    candidates.sort_unstable_by(|left, right| {
        right
            .calls
            .cmp(&left.calls)
            .then_with(|| left.name.cmp(&right.name))
    });
    egui::CollapsingHeader::new(format!("Discovered code ({})", candidates.len()))
        .default_open(true)
        .show(ui, |ui| {
            for candidate in candidates.iter().take(100) {
                draw_runtime_candidate(ui, state, candidate, actions);
            }
            if candidates.len() > 100 {
                ui.weak(format!("{} more", candidates.len() - 100));
            }
        });
}

fn draw_runtime_candidate(
    ui: &mut egui::Ui,
    state: &mut SymbolBrowserState,
    candidate: &RuntimeSymbolCandidate,
    actions: &mut DebugUiActions,
) {
    ui.horizontal_wrapped(|ui| {
        ui.monospace(&candidate.name);
        ui.weak(format!(
            "{} calls | {:?} | {:?}",
            candidate.calls, candidate.provenance, candidate.confidence
        ));
        if let Some(storage) = candidate.location.storage {
            ui.monospace(format!("ROM {:X}", storage.offset));
        }
        if ui.small_button("Code").clicked()
            && let Some(cpu) = candidate.location.cpu
            && let Ok(cpu_address) = u32::try_from(cpu.address)
        {
            actions.disasm_target = Some(DisassemblyTarget {
                cpu_address,
                storage_offset: candidate.location.storage.map(|storage| storage.offset),
                bank: candidate.location.bank,
                thumb: match candidate.location.exec_mode {
                    ExecMode::Thumb => Some(true),
                    ExecMode::Arm => Some(false),
                    _ => None,
                },
            });
            actions.focus_tab = Some(DebugTab::Disassembler);
        }
        if ui.small_button("Promote").clicked() {
            state.editor = Some(SymbolEditorState {
                original_user_name: None,
                draft: UserSymbolDraft {
                    name: candidate.name.clone(),
                    location: candidate.location,
                    value: None,
                    kind: SymbolKind::Function,
                    size: None,
                    comment: None,
                },
            });
        }
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
    symbols: &SymbolSession,
    symbol: &SymbolRecord,
    hits: u64,
) {
    ui.separator();
    ui.label(
        egui::RichText::new(&symbol.name)
            .monospace()
            .strong()
            .color(crate::debug::common::color32(
                crate::debug::common::debug_colors(ui).symbol,
            )),
    );
    egui::Grid::new("selected_symbol_details")
        .num_columns(2)
        .spacing([10.0, 2.0])
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
                if let Some(cpu) = symbols.active_runtime_cpu_for_storage(storage) {
                    detail_row(ui, "Runtime CPU", format!("{:X}", cpu.address));
                }
            }
            if let Some(value) = symbol.value {
                detail_row(ui, "Value", format!("{value:X}"));
            }
            if let Some(size) = symbol.size {
                detail_row(ui, "Size", format!("{size:X}"));
            }
            detail_row(ui, "Source", format!("{:?}", symbol.provenance.kind));
            detail_row(ui, "Executed", format!("{hits} instructions"));
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
    ui.separator();
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
    hits: u64,
    selected: bool,
    row: &mut SymbolRowContext<'_>,
) -> bool {
    let mut clicked = false;
    ui.horizontal_wrapped(|ui| {
        clicked = ui.selectable_label(selected, &symbol.name).clicked();
        ui.weak(format!("{:?}", symbol.kind));
        if hits != 0 {
            ui.colored_label(
                crate::debug::common::color32(crate::debug::common::debug_colors(ui).changed),
                format!("{hits} hits"),
            );
        }
        if let Some(bank) = symbol.location.bank {
            ui.monospace(format!("B{bank:X}"));
        }
        let runtime_cpu = symbol
            .location
            .storage
            .and_then(|storage| row.symbols.active_runtime_cpu_for_storage(storage));
        if let Some(cpu) = runtime_cpu.or(symbol.location.cpu) {
            ui.monospace(format!("CPU {:X}", cpu.address));
            if (runtime_cpu.is_some()
                || symbol.location.exec_mode == ExecMode::Sm83 && cpu.address < 0x8000
                || symbol.location.bank.is_some()
                || row.exec_mode == ExecMode::Arm
                    && (0x0800_0000..=0x0DFF_FFFF).contains(&cpu.address)
                || symbol.location.exec_mode == ExecMode::V30 && cpu.address <= 0x0F_FFFF)
                && let Ok(cpu_address) = u32::try_from(cpu.address)
                && ui.small_button("Code").clicked()
            {
                row.actions.disasm_target = Some(DisassemblyTarget {
                    cpu_address,
                    storage_offset: symbol.location.storage.map(|storage| storage.offset),
                    bank: symbol.location.bank,
                    thumb: match symbol.location.exec_mode {
                        ExecMode::Thumb => Some(true),
                        ExecMode::Arm => Some(false),
                        _ => None,
                    },
                });
                row.actions.focus_tab = Some(DebugTab::Disassembler);
            }
            if let Some(storage) = symbol.location.storage {
                let physical = row.exec_mode == ExecMode::Sm83 && runtime_cpu.is_none();
                let is_set = row.cpu_debug.is_some_and(|info| {
                    if physical {
                        info.rom_breakpoints.contains(&storage.offset)
                    } else {
                        u32::try_from(cpu.address)
                            .is_ok_and(|address| info.breakpoints.contains(&address))
                    }
                });
                let label = if is_set { "Unbreak" } else { "Break" };
                if ui.small_button(label).clicked() {
                    if physical {
                        row.actions.toggle_rom_breakpoints.push(storage.offset);
                    } else if let Ok(address) = u32::try_from(cpu.address) {
                        row.actions.toggle_breakpoints.push(address);
                    }
                }
            } else if let Ok(address) = u32::try_from(cpu.address) {
                let is_set = row
                    .cpu_debug
                    .is_some_and(|info| info.breakpoints.contains(&address));
                let label = if is_set { "Unbreak" } else { "Break" };
                if ui.small_button(label).clicked() {
                    row.actions.toggle_breakpoints.push(address);
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
