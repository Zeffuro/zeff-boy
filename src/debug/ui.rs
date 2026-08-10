use std::fmt::Write;

use crate::debug::common::{COLOR_CONTINUE_BUTTON, WatchType, format_addr};
use crate::debug::types::CpuDebugSnapshot;
use zeff_emu_common::address::Address;

pub(crate) use super::menu_bar::{MenuAction, MenuBarContext, MenuBarResult, draw_menu_bar};
pub(crate) use super::settings_window::{SettingsContext, draw_settings_window};

pub(crate) struct DebugUiActions {
    pub(crate) add_breakpoint: Option<Address>,
    pub(crate) add_watchpoint: Option<(Address, WatchType)>,
    pub(crate) remove_breakpoints: Vec<Address>,
    pub(crate) toggle_breakpoints: Vec<Address>,
    pub(crate) memory_writes: Vec<(Address, u8)>,
    pub(crate) apu_channel_mutes: Option<Vec<bool>>,
    pub(crate) step_requested: bool,
    pub(crate) continue_requested: bool,
    pub(crate) backstep_requested: bool,
    pub(crate) layer_toggles: Option<(bool, bool, bool)>,
    pub(crate) gba_bg_layer_toggles: Option<[bool; 4]>,
}

impl DebugUiActions {
    pub(crate) fn none() -> Self {
        Self {
            add_breakpoint: None,
            add_watchpoint: None,
            remove_breakpoints: Vec::new(),
            toggle_breakpoints: Vec::new(),
            memory_writes: Vec::new(),
            apu_channel_mutes: None,
            step_requested: false,
            continue_requested: false,
            backstep_requested: false,
            layer_toggles: None,
            gba_bg_layer_toggles: None,
        }
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.add_breakpoint.is_some()
            || self.add_watchpoint.is_some()
            || !self.remove_breakpoints.is_empty()
            || !self.toggle_breakpoints.is_empty()
            || !self.memory_writes.is_empty()
            || self.apu_channel_mutes.is_some()
            || self.layer_toggles.is_some()
            || self.gba_bg_layer_toggles.is_some()
    }
}

/// Unified CPU / System debug panel. Renders any console's snapshot.
pub(super) fn draw_cpu_debug_content(
    ui: &mut egui::Ui,
    info: &CpuDebugSnapshot,
    actions: &mut DebugUiActions,
) {
    ui.heading("CPU Registers");
    for line in &info.register_lines {
        ui.monospace(line);
    }
    ui.separator();

    ui.heading("Flags");
    let flags_str: String = info
        .flags
        .iter()
        .map(|(ch, set)| if *set { *ch } else { '-' })
        .collect();
    ui.monospace(format!("[{}]  {}", flags_str, info.status_text));
    ui.separator();

    ui.heading("Last Opcode");
    ui.monospace(&info.last_opcode_line);
    ui.monospace(format!("Total cycles: {}", info.cycles));
    ui.separator();

    for section in &info.sections {
        ui.heading(section.heading);
        for line in &section.lines {
            ui.monospace(line);
        }
        ui.separator();
    }

    ui.heading("Memory @ PC");
    let mut line = String::new();
    for (i, (addr, val)) in info.mem_around_pc.iter().enumerate() {
        if i % 8 == 0 {
            if !line.is_empty() {
                ui.monospace(&line);
                line.clear();
            }
            let _ = write!(line, "{}: ", format_addr(*addr));
        }
        let _ = write!(line, "{:02X} ", val);
    }
    if !line.is_empty() {
        ui.monospace(&line);
    }

    if !info.recent_opcodes.is_empty() {
        ui.separator();
        ui.heading("Recent Opcodes");
        for opcode in &info.recent_opcodes {
            ui.monospace(opcode.line());
        }
    }

    let suspended = info.cpu_state == "Suspended";
    if suspended {
        ui.separator();
        let button = egui::Button::new("▶ Continue (F5)").fill(COLOR_CONTINUE_BUTTON);
        if ui.add(button).clicked() {
            actions.continue_requested = true;
        }
    }

    ui.horizontal(|ui| {
        if ui.button("Step").clicked() {
            actions.step_requested = true;
        }
        if !suspended && ui.button("Continue").clicked() {
            actions.continue_requested = true;
        }
    });

    if let Some(bp) = info.hit_breakpoint {
        ui.monospace(format!("Hit breakpoint @ {}", format_addr(bp)));
    }
    if let Some(hit) = &info.hit_watchpoint {
        ui.monospace(format!(
            "Watch hit: {:?} @ {}: {:02X} -> {:02X}",
            hit.watch_type,
            format_addr(hit.address),
            hit.old_value,
            hit.new_value
        ));
    }
}
