use std::collections::VecDeque;

use crate::debug::{
    CpuDebugSnapshot, DebugUiActions, DisassemblyView, MemoryViewerState, RomDebugInfo,
    RomViewerState,
};
use crate::symbols::SymbolSession;
use zeff_emu_common::address::Address;

mod commands;

use commands::{CommandContext, complete_pending_read, completions, run_command};

const OUTPUT_LIMIT: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConsoleReadSpace {
    Cpu,
    Rom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PendingConsoleRead {
    pub(crate) space: ConsoleReadSpace,
    pub(crate) start: u32,
    pub(crate) length: usize,
}

pub(crate) struct DebugConsoleState {
    input: String,
    output: VecDeque<String>,
    history: Vec<String>,
    history_index: Option<usize>,
    pub(crate) pending_read: Option<PendingConsoleRead>,
    guest_call_pending: bool,
    guest_call_undo: Option<Vec<u8>>,
}

impl Default for DebugConsoleState {
    fn default() -> Self {
        let mut output = VecDeque::new();
        output.push_back("Debug Console - type help for commands".to_owned());
        Self {
            input: String::new(),
            output,
            history: Vec::new(),
            history_index: None,
            pending_read: None,
            guest_call_pending: false,
            guest_call_undo: None,
        }
    }
}

pub(super) struct DebugConsoleViews<'a> {
    pub(super) memory: &'a mut MemoryViewerState,
    pub(super) rom: &'a mut RomViewerState,
}

pub(super) struct DebugConsoleContext<'a> {
    pub(super) symbols: &'a SymbolSession,
    pub(super) cpu_debug: Option<&'a CpuDebugSnapshot>,
    pub(super) rom_debug: Option<&'a RomDebugInfo>,
    pub(super) disassembly: Option<&'a DisassemblyView>,
    pub(super) memory_page: Option<&'a [(Address, u8)]>,
    pub(super) rom_page: Option<&'a [(u32, u8)]>,
}

pub(super) fn draw_debug_console_content(
    ui: &mut egui::Ui,
    state: &mut DebugConsoleState,
    context: DebugConsoleContext<'_>,
    views: DebugConsoleViews<'_>,
    actions: &mut DebugUiActions,
) {
    complete_pending_read(state, context.memory_page, context.rom_page);

    let output_height = (ui.available_height() - 58.0).max(80.0);
    egui::ScrollArea::vertical()
        .max_height(output_height)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in &state.output {
                ui.monospace(line);
            }
        });
    ui.separator();

    let suggestions = completions(&state.input, context.symbols);
    let mut submit = false;
    ui.horizontal(|ui| {
        ui.monospace(">");
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.input)
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Monospace),
        );
        submit = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if response.has_focus() {
            if ui.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                history_previous(state);
            } else if ui.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                history_next(state);
            } else if ui.input(|input| input.key_pressed(egui::Key::Tab))
                && let Some(first) = suggestions.first()
            {
                state.input.clone_from(first);
            }
        }
        submit |= ui.button("Run").clicked();
    });

    if !suggestions.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for suggestion in suggestions.iter().take(8) {
                if ui.small_button(suggestion).clicked() {
                    state.input.clone_from(suggestion);
                }
            }
        });
    }

    if submit {
        let input = state.input.trim().to_owned();
        state.input.clear();
        run_command(
            state,
            &input,
            CommandContext {
                symbols: context.symbols,
                cpu_debug: context.cpu_debug,
                rom_debug: context.rom_debug,
                disassembly: context.disassembly,
                views,
                actions,
            },
        );
    }
}

fn history_previous(state: &mut DebugConsoleState) {
    if state.history.is_empty() {
        return;
    }
    let index = state
        .history_index
        .map_or(state.history.len() - 1, |index| index.saturating_sub(1));
    state.history_index = Some(index);
    state.input.clone_from(&state.history[index]);
}

fn history_next(state: &mut DebugConsoleState) {
    let Some(index) = state.history_index else {
        return;
    };
    if index + 1 < state.history.len() {
        state.history_index = Some(index + 1);
        state.input.clone_from(&state.history[index + 1]);
    } else {
        state.history_index = None;
        state.input.clear();
    }
}

impl DebugConsoleState {
    pub(crate) fn guest_call_completed(
        &mut self,
        name: &str,
        instructions: u64,
        undo_state: Vec<u8>,
    ) {
        self.guest_call_pending = false;
        self.guest_call_undo = Some(undo_state);
        self.push(format!("{name} returned after {instructions} instructions"));
    }

    pub(crate) fn guest_call_failed(&mut self, name: &str, error: &str) {
        self.guest_call_pending = false;
        self.push(format!("{name} failed: {error}"));
    }

    pub(crate) fn guest_call_undone(&mut self) {
        self.guest_call_pending = false;
        self.guest_call_undo = None;
        self.push("Guest call undone".to_owned());
    }

    pub(crate) fn guest_call_undo_failed(&mut self, error: &str) {
        self.guest_call_pending = false;
        self.push(format!("Could not undo guest call: {error}"));
    }

    fn push(&mut self, line: String) {
        self.output.push_back(line);
        while self.output.len() > OUTPUT_LIMIT {
            self.output.pop_front();
        }
    }
}

#[cfg(test)]
mod tests;
