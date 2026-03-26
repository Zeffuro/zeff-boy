use crate::debug::common::{CpuDebugSnapshot, WatchType};

pub(crate) use super::menu_bar::{MenuActions, draw_menu_bar};
pub(crate) use super::settings_window::draw_settings_window;

pub(crate) struct DebugUiActions {
    pub(crate) add_breakpoint: Option<u16>,
    pub(crate) add_watchpoint: Option<(u16, WatchType)>,
    pub(crate) remove_breakpoints: Vec<u16>,
    pub(crate) toggle_breakpoints: Vec<u16>,
    pub(crate) memory_writes: Vec<(u16, u8)>,
    pub(crate) apu_channel_mutes: Option<Vec<bool>>,
    pub(crate) step_requested: bool,
    pub(crate) continue_requested: bool,
    pub(crate) backstep_requested: bool,
    pub(crate) layer_toggles: Option<(bool, bool, bool)>,
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
        ui.heading(&section.heading);
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
            line.push_str(&format!("{:04X}: ", addr));
        }
        line.push_str(&format!("{:02X} ", val));
    }
    if !line.is_empty() {
        ui.monospace(&line);
    }

    if !info.recent_op_lines.is_empty() {
        ui.separator();
        ui.heading("Recent Opcodes");
        for op_line in &info.recent_op_lines {
            ui.monospace(op_line);
        }
    }

    let suspended = info.cpu_state == "Suspended";
    if suspended {
        ui.separator();
        let button =
            egui::Button::new("▶ Continue (F5)").fill(egui::Color32::from_rgb(40, 100, 40));
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
        ui.monospace(format!("Hit breakpoint @ {:04X}", bp));
    }
    if let Some(hit) = &info.hit_watchpoint {
        ui.monospace(format!(
            "Watch hit: {:?} @ {:04X}: {:02X} -> {:02X}",
            hit.watch_type, hit.address, hit.old_value, hit.new_value
        ));
    }
}
