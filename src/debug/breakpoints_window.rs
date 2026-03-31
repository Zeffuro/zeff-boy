use crate::debug::BreakpointState;
use crate::debug::common::{
    COLOR_BREAKPOINT_HIT, COLOR_CONTINUE_BUTTON, COLOR_WATCHPOINT_HIT, WatchType, parse_hex_u16,
};
use crate::debug::types::CpuDebugSnapshot;
use crate::debug::ui::DebugUiActions;

pub(super) fn draw_breakpoints_content(
    ui: &mut egui::Ui,
    info: &CpuDebugSnapshot,
    state: &mut BreakpointState,
    actions: &mut DebugUiActions,
) {
    ui.heading("Breakpoints");
    ui.horizontal(|ui| {
        ui.label("Address:");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut state.input)
                .desired_width(80.0)
                .hint_text("hex addr"),
        );
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Add").clicked() || enter)
            && let Some(addr) = parse_hex_u16(&state.input)
        {
            actions.add_breakpoint = Some(addr);
            state.input.clear();
        }
    });

    if info.breakpoints.is_empty() {
        ui.label(
            egui::RichText::new("No breakpoints set.")
                .italics()
                .color(egui::Color32::GRAY),
        );
    } else {
        egui::Grid::new("bp_grid")
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Address").strong());
                ui.label(egui::RichText::new("Actions").strong());
                ui.end_row();

                let mut remove_addr = None;
                let mut toggle_addr = None;
                for &addr in &info.breakpoints {
                    let hit = info.hit_breakpoint == Some(addr);
                    let label = if hit {
                        egui::RichText::new(format!("{:04X} ●", addr))
                            .color(COLOR_BREAKPOINT_HIT)
                            .monospace()
                    } else {
                        egui::RichText::new(format!("{:04X}", addr)).monospace()
                    };
                    ui.label(label);
                    ui.horizontal(|ui| {
                        if ui.small_button("Toggle").clicked() {
                            toggle_addr = Some(addr);
                        }
                        if ui.small_button("Remove").clicked() {
                            remove_addr = Some(addr);
                        }
                    });
                    ui.end_row();
                }
                if let Some(addr) = toggle_addr {
                    actions.toggle_breakpoints.push(addr);
                }
                if let Some(addr) = remove_addr {
                    actions.remove_breakpoints.push(addr);
                }
            });
    }

    ui.separator();
    ui.heading("Watchpoints");
    ui.horizontal(|ui| {
        ui.label("Address:");
        ui.add(
            egui::TextEdit::singleline(&mut state.watchpoint_input)
                .desired_width(80.0)
                .hint_text("hex addr"),
        );
        egui::ComboBox::from_id_salt("wp_type_bp_window")
            .width(90.0)
            .selected_text(match state.watchpoint_type {
                WatchType::Read => "Read",
                WatchType::Write => "Write",
                WatchType::ReadWrite => "R/W",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.watchpoint_type, WatchType::Read, "Read");
                ui.selectable_value(&mut state.watchpoint_type, WatchType::Write, "Write");
                ui.selectable_value(&mut state.watchpoint_type, WatchType::ReadWrite, "R/W");
            });
        if ui.button("Add").clicked()
            && let Some(addr) = parse_hex_u16(&state.watchpoint_input)
        {
            actions.add_watchpoint = Some((addr, state.watchpoint_type));
            state.watchpoint_input.clear();
        }
    });

    if info.watchpoints.is_empty() {
        ui.label(
            egui::RichText::new("No watchpoints set.")
                .italics()
                .color(egui::Color32::GRAY),
        );
    } else {
        egui::Grid::new("wp_grid")
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Address").strong());
                ui.label(egui::RichText::new("Type").strong());
                ui.end_row();

                for wp in &info.watchpoints {
                    let hit = info
                        .hit_watchpoint
                        .as_ref()
                        .is_some_and(|h| h.address == wp.address);
                    let label = if hit {
                        egui::RichText::new(format!("{:04X} ●", wp.address))
                            .color(COLOR_WATCHPOINT_HIT)
                            .monospace()
                    } else {
                        egui::RichText::new(format!("{:04X}", wp.address)).monospace()
                    };
                    ui.label(label);
                    ui.monospace(format!("{:?}", wp.watch_type));
                    ui.end_row();
                }
            });
    }

    if let Some(bp) = info.hit_breakpoint {
        ui.separator();
        ui.colored_label(
            COLOR_BREAKPOINT_HIT,
            format!("⚑ Breakpoint hit @ {:04X}", bp),
        );
    }
    if let Some(ref hit) = info.hit_watchpoint {
        ui.separator();
        ui.colored_label(
            COLOR_WATCHPOINT_HIT,
            format!(
                "⚑ Watchpoint {:?} @ {:04X}: {:02X} → {:02X}",
                hit.watch_type, hit.address, hit.old_value, hit.new_value
            ),
        );
    }

    let suspended = info.cpu_state == "Suspended";
    if suspended {
        ui.separator();
        let button = egui::Button::new("▶ Continue (F5)").fill(COLOR_CONTINUE_BUTTON);
        if ui.add(button).clicked() {
            actions.continue_requested = true;
        }
        if ui.button("Step (F10)").clicked() {
            actions.step_requested = true;
        }
    }
}
