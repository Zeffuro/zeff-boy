use crate::debug::types::{MemoryViewerState, RomViewerState, SymbolBrowserState};
use crate::debug::{CpuDebugSnapshot, DebugTab, DebugUiActions, DisassemblyTarget};
use crate::symbols::ExecMode;
use crate::symbols::{SymbolRecord, SymbolSession};

const MAX_RESULTS: usize = 1000;

pub(super) struct SymbolBrowserViews<'a> {
    pub(super) state: &'a mut SymbolBrowserState,
    pub(super) memory: &'a mut MemoryViewerState,
    pub(super) rom: &'a mut RomViewerState,
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
        ui.label("No symbols loaded");
        return;
    }

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

    if state.last_query != state.query || state.last_generation != symbols.store.generation() {
        state.results = symbols.store.search_ids(&state.query, MAX_RESULTS);
        state.last_query.clone_from(&state.query);
        state.last_generation = symbols.store.generation();
    }

    ui.label(format!(
        "{} shown / {} loaded",
        state.results.len(),
        symbols.symbol_count()
    ));
    ui.separator();

    for id in state.results.iter().copied() {
        let Some(symbol) = symbols.store.symbol(id) else {
            continue;
        };
        draw_symbol_row(ui, symbol, cpu_debug, memory, rom, actions);
    }
}

fn draw_symbol_row(
    ui: &mut egui::Ui,
    symbol: &SymbolRecord,
    cpu_debug: Option<&CpuDebugSnapshot>,
    memory: &mut MemoryViewerState,
    rom: &mut RomViewerState,
    actions: &mut DebugUiActions,
) {
    ui.horizontal_wrapped(|ui| {
        ui.monospace(&symbol.name);
        if let Some(cpu) = symbol.location.cpu {
            ui.monospace(format!("CPU {:X}", cpu.address));
            if symbol.location.exec_mode == ExecMode::Sm83
                && cpu.address < 0x8000
                && let Some(storage) = symbol.location.storage
                && let Ok(cpu_address) = u32::try_from(cpu.address)
                && ui.small_button("Code").clicked()
            {
                actions.disasm_target = Some(DisassemblyTarget {
                    cpu_address,
                    storage_offset: storage.offset,
                });
                actions.focus_tab = Some(DebugTab::Disassembler);
            }
            if let Some(storage) = symbol.location.storage {
                let is_set =
                    cpu_debug.is_some_and(|info| info.rom_breakpoints.contains(&storage.offset));
                let label = if is_set { "Unbreak" } else { "Break" };
                if ui.small_button(label).clicked() {
                    actions.toggle_rom_breakpoints.push(storage.offset);
                }
            }
            if ui.small_button("Memory").clicked()
                && let Ok(address) = u32::try_from(cpu.address)
            {
                memory.view_start = memory.address_space.clamp_start(address);
                memory.jump_input = memory.address_space.format(memory.view_start);
                actions.focus_tab = Some(DebugTab::MemoryViewer);
            }
        }
        if let Some(storage) = symbol.location.storage {
            ui.monospace(format!("ROM {:X}", storage.offset));
            if ui.small_button("ROM").clicked()
                && let Ok(offset) = u32::try_from(storage.offset)
            {
                rom.view_start = offset & !0xF;
                rom.jump_input = format!("{:06X}", rom.view_start);
                actions.focus_tab = Some(DebugTab::RomViewer);
            }
        }
    });
}
