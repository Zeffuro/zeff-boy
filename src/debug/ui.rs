use crate::debug::common::{COLOR_CONTINUE_BUTTON, WatchType, format_addr};
use crate::debug::types::{CpuDebugSnapshot, CpuDebugViewState};
use zeff_emu_common::address::Address;

pub(crate) use super::menu_bar::{MenuAction, MenuBarContext, MenuBarResult, draw_menu_bar};
#[cfg(target_arch = "wasm32")]
pub(crate) use super::settings_window::draw_settings_window;
pub(crate) use super::settings_window::{SettingsContext, draw_settings_content};

pub(crate) struct DebugUiActions {
    pub(crate) add_breakpoint: Option<Address>,
    pub(crate) add_one_shot_breakpoint: Option<Address>,
    pub(crate) add_breakpoint_after: Option<(Address, u64)>,
    pub(crate) event_breakpoint_changes: Vec<(zeff_emu_common::debug::DebugEvent, bool)>,
    pub(crate) add_watchpoint: Option<(Address, Address, WatchType)>,
    pub(crate) remove_watchpoints: Vec<(Address, Address, WatchType)>,
    pub(crate) remove_breakpoints: Vec<Address>,
    pub(crate) toggle_breakpoints: Vec<Address>,
    pub(crate) remove_rom_breakpoints: Vec<u64>,
    pub(crate) add_rom_breakpoints: Vec<u64>,
    pub(crate) toggle_rom_breakpoints: Vec<u64>,
    pub(crate) memory_writes: Vec<(Address, u8)>,
    pub(crate) apu_channel_mutes: Option<Vec<bool>>,
    pub(crate) step_requested: bool,
    pub(crate) next_frame_requested: bool,
    pub(crate) continue_requested: bool,
    pub(crate) backstep_requested: bool,
    pub(crate) layer_toggles: Option<(bool, bool, bool)>,
    pub(crate) gba_bg_layer_toggles: Option<[bool; 4]>,
    pub(crate) focus_tab: Option<super::DebugTab>,
    pub(crate) memory_target: Option<Address>,
    pub(crate) disasm_target: Option<super::DisassemblyTarget>,
    pub(crate) follow_disasm_pc: bool,
    pub(crate) disasm_back: bool,
    pub(crate) disasm_forward: bool,
    pub(crate) user_symbol: Option<crate::symbols::UserSymbolDraft>,
    pub(crate) remove_user_symbols: Vec<String>,
    pub(crate) trace_enabled: Option<bool>,
    pub(crate) trace_clear: bool,
    pub(crate) trace_capacity: Option<usize>,
    pub(crate) guest_call: Option<crate::emu_thread::GuestCallRequest>,
    pub(crate) undo_guest_call: Option<Vec<u8>>,
}

impl DebugUiActions {
    pub(crate) fn none() -> Self {
        Self {
            add_breakpoint: None,
            add_one_shot_breakpoint: None,
            add_breakpoint_after: None,
            event_breakpoint_changes: Vec::new(),
            add_watchpoint: None,
            remove_watchpoints: Vec::new(),
            remove_breakpoints: Vec::new(),
            toggle_breakpoints: Vec::new(),
            remove_rom_breakpoints: Vec::new(),
            add_rom_breakpoints: Vec::new(),
            toggle_rom_breakpoints: Vec::new(),
            memory_writes: Vec::new(),
            apu_channel_mutes: None,
            step_requested: false,
            next_frame_requested: false,
            continue_requested: false,
            backstep_requested: false,
            layer_toggles: None,
            gba_bg_layer_toggles: None,
            focus_tab: None,
            memory_target: None,
            disasm_target: None,
            follow_disasm_pc: false,
            disasm_back: false,
            disasm_forward: false,
            user_symbol: None,
            remove_user_symbols: Vec::new(),
            trace_enabled: None,
            trace_clear: false,
            trace_capacity: None,
            guest_call: None,
            undo_guest_call: None,
        }
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.add_breakpoint.is_some()
            || self.add_one_shot_breakpoint.is_some()
            || self.add_breakpoint_after.is_some()
            || !self.event_breakpoint_changes.is_empty()
            || self.add_watchpoint.is_some()
            || !self.remove_watchpoints.is_empty()
            || !self.remove_breakpoints.is_empty()
            || !self.toggle_breakpoints.is_empty()
            || !self.remove_rom_breakpoints.is_empty()
            || !self.add_rom_breakpoints.is_empty()
            || !self.toggle_rom_breakpoints.is_empty()
            || !self.memory_writes.is_empty()
            || self.apu_channel_mutes.is_some()
            || self.layer_toggles.is_some()
            || self.gba_bg_layer_toggles.is_some()
            || self.trace_enabled.is_some()
            || self.trace_clear
            || self.trace_capacity.is_some()
    }

    pub(crate) fn requires_emulator_authority(&self) -> bool {
        self.has_pending()
            || self.step_requested
            || self.next_frame_requested
            || self.continue_requested
            || self.backstep_requested
            || self.guest_call.is_some()
            || self.undo_guest_call.is_some()
    }
}

pub(super) fn draw_cpu_debug_content(
    ui: &mut egui::Ui,
    info: &CpuDebugSnapshot,
    state: &mut CpuDebugViewState,
    actions: &mut DebugUiActions,
    supports_rewind: bool,
    supports_execution_controls: bool,
) {
    state.sync(info);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        draw_cpu_debug_rows(
            ui,
            info,
            state,
            actions,
            supports_rewind,
            supports_execution_controls,
        );
    });
}

fn draw_cpu_debug_rows(
    ui: &mut egui::Ui,
    info: &CpuDebugSnapshot,
    state: &CpuDebugViewState,
    actions: &mut DebugUiActions,
    supports_rewind: bool,
    supports_execution_controls: bool,
) {
    let colors = crate::debug::common::debug_colors(ui);
    let changed = crate::debug::common::color32(colors.changed);
    let address = crate::debug::common::color32(colors.address);
    let active = crate::debug::common::color32(colors.symbol);
    let breakpoint = crate::debug::common::color32(colors.breakpoint);
    let watchpoint = crate::debug::common::color32(colors.watchpoint);
    let selection = crate::debug::common::color32(colors.selection);
    let mono = crate::debug::common::debug_mono_font(ui);
    let suspended = info.cpu_state.eq_ignore_ascii_case("Suspended");
    let running = info.cpu_state.eq_ignore_ascii_case("Running");
    let state_color = if suspended {
        breakpoint
    } else if running {
        active
    } else {
        selection
    };

    ui.horizontal_wrapped(|ui| {
        if suspended {
            let button = egui::Button::new("Continue (F5)").fill(COLOR_CONTINUE_BUTTON);
            if ui
                .add_enabled(supports_execution_controls, button)
                .clicked()
            {
                actions.continue_requested = true;
            }
        }
        if ui
            .add_enabled(
                supports_execution_controls && suspended,
                egui::Button::new("Step (F7)"),
            )
            .clicked()
        {
            actions.step_requested = true;
        }
        if ui
            .add_enabled(supports_execution_controls, egui::Button::new("Next Frame"))
            .clicked()
        {
            actions.next_frame_requested = true;
        }
        if ui
            .add_enabled(supports_rewind, egui::Button::new("Step Back"))
            .clicked()
        {
            actions.backstep_requested = true;
        }
        ui.separator();
        if ui.small_button("Disassembly").clicked() {
            actions.focus_tab = Some(super::DebugTab::Disassembler);
        }
        if ui.small_button("History").clicked() {
            actions.focus_tab = Some(super::DebugTab::ExecutionHistory);
        }
        if ui.small_button("Trace").clicked() {
            actions.focus_tab = Some(super::DebugTab::Trace);
        }
        if ui.small_button("Memory").clicked() {
            actions.focus_tab = Some(super::DebugTab::MemoryViewer);
        }
        if ui.small_button("Hardware / I/O").clicked() {
            actions.focus_tab = Some(super::DebugTab::HardwareIo);
        }
    });

    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(&info.cpu_state)
                .font(mono.clone())
                .color(state_color)
                .strong(),
        );
        ui.label(
            egui::RichText::new(format!("PC {}", format_addr(info.pc)))
                .font(mono.clone())
                .color(selection),
        );
        ui.weak(&info.status_text);
    });

    if let Some(bp) = info.hit_breakpoint {
        ui.colored_label(
            breakpoint,
            egui::RichText::new(format!("Breakpoint hit at {}", format_addr(bp)))
                .font(mono.clone()),
        );
    }
    if let Some(offset) = info.hit_rom_breakpoint {
        ui.colored_label(
            breakpoint,
            egui::RichText::new(format!("ROM breakpoint hit at +{offset:06X}")).font(mono.clone()),
        );
    }
    if let Some(hit) = &info.hit_watchpoint {
        ui.colored_label(
            watchpoint,
            egui::RichText::new(format!(
                "Watch hit {:?} at {}: {:02X} to {:02X}",
                hit.watch_type,
                format_addr(hit.address),
                hit.old_value,
                hit.new_value
            ))
            .font(mono.clone()),
        );
    }
    if let Some(event) = info.hit_event {
        ui.colored_label(
            breakpoint,
            egui::RichText::new(format!("{} breakpoint hit", event.label())).font(mono.clone()),
        );
    }

    ui.separator();
    ui.weak("Registers");
    for (index, line) in info.register_lines.iter().enumerate() {
        let text = egui::RichText::new(line).font(mono.clone());
        if state.register_changed(index) {
            ui.colored_label(changed, text);
        } else {
            ui.label(text);
        }
    }
    ui.horizontal(|ui| {
        ui.weak("Flags");
        for (index, (ch, set)) in info.flags.iter().enumerate() {
            let text = egui::RichText::new(ch.to_string())
                .font(mono.clone())
                .strong();
            if state.flag_changed(index) {
                ui.colored_label(changed, text);
            } else if *set {
                ui.colored_label(active, text);
            } else {
                ui.weak(text);
            }
        }
    });
    ui.separator();

    ui.horizontal_wrapped(|ui| {
        ui.weak("Last");
        ui.label(
            egui::RichText::new(&info.last_opcode_line)
                .font(mono.clone())
                .color(address),
        );
        ui.weak(format!("{} cycles", info.cycles));
    });
}

#[cfg(test)]
mod tests {
    use super::DebugUiActions;

    #[test]
    fn emulator_debug_actions_are_distinct_from_local_navigation() {
        let mut actions = DebugUiActions::none();
        actions.follow_disasm_pc = true;
        assert!(!actions.requires_emulator_authority());

        actions.step_requested = true;
        assert!(actions.requires_emulator_authority());
        actions.step_requested = false;
        actions.backstep_requested = true;
        assert!(actions.requires_emulator_authority());
        actions.backstep_requested = false;
        actions.trace_clear = true;
        assert!(actions.requires_emulator_authority());
    }
}
