use crate::debug::DebugInfo;
use crate::debug::breakpoints::WatchType;

pub(crate) use super::menu_bar::{MenuActions, draw_menu_bar};
pub(crate) use super::settings_window::draw_settings_window;

pub(crate) struct DebugUiActions {
    pub(crate) add_breakpoint: Option<u16>,
    pub(crate) add_watchpoint: Option<(u16, WatchType)>,
    pub(crate) remove_breakpoints: Vec<u16>,
    pub(crate) toggle_breakpoints: Vec<u16>,
    pub(crate) memory_writes: Vec<(u16, u8)>,
    pub(crate) apu_channel_mutes: Option<[bool; 4]>,
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

pub(super) fn draw_debug_ui_content(
    ui: &mut egui::Ui,
    info: &DebugInfo,
    actions: &mut DebugUiActions,
) {
            ui.heading("Performance");
            ui.label(format!("FPS: {:.1} ({})", info.fps, info.speed_mode_label));
            ui.label(format!("Total cycles: {}", info.cycles));
            ui.monospace(format!(
                "HW mode: {:?} (pref: {:?})",
                info.hardware_mode, info.hardware_mode_preference
            ));

            ui.separator();

            ui.heading("CPU Registers");
            ui.horizontal(|ui| {
                ui.monospace(format!("A:{:02X}  F:{:02X}", info.a, info.f));
                ui.monospace(format!("  AF:{:04X}", (info.a as u16) << 8 | info.f as u16));
            });
            ui.horizontal(|ui| {
                ui.monospace(format!("B:{:02X}  C:{:02X}", info.b, info.c));
                ui.monospace(format!("  BC:{:04X}", (info.b as u16) << 8 | info.c as u16));
            });
            ui.horizontal(|ui| {
                ui.monospace(format!("D:{:02X}  E:{:02X}", info.d, info.e));
                ui.monospace(format!("  DE:{:04X}", (info.d as u16) << 8 | info.e as u16));
            });
            ui.horizontal(|ui| {
                ui.monospace(format!("H:{:02X}  L:{:02X}", info.h, info.l));
                ui.monospace(format!("  HL:{:04X}", (info.h as u16) << 8 | info.l as u16));
            });
            ui.monospace(format!("PC:{:04X}  SP:{:04X}", info.pc, info.sp));
            ui.separator();

            ui.heading("Flags");
            let z = if info.f & 0x80 != 0 { "Z" } else { "-" };
            let n = if info.f & 0x40 != 0 { "N" } else { "-" };
            let h = if info.f & 0x20 != 0 { "H" } else { "-" };
            let c = if info.f & 0x10 != 0 { "C" } else { "-" };
            ui.monospace(format!(
                "[{}{}{}{}]  IME: {}  State: {}",
                z, n, h, c, info.ime, info.cpu_state
            ));
            ui.separator();

            ui.heading("Last Opcode");
            ui.monospace(format!(
                "@ {:04X} = {:02X}",
                info.last_opcode_pc, info.last_opcode
            ));
            ui.separator();

            ui.heading("Interrupts");
            ui.monospace(format!(
                "IF:{:02X}  IE:{:02X}  pending:{:02X}",
                info.if_reg,
                info.ie,
                info.if_reg & info.ie
            ));
            let int_names = ["VBlank", "STAT", "Timer", "Serial", "Joypad"];
            ui.horizontal_wrapped(|ui| {
                for (i, name) in int_names.iter().enumerate() {
                    let ie = if info.ie & (1 << i) != 0 { "E" } else { "." };
                    let ifr = if info.if_reg & (1 << i) != 0 {
                        "F"
                    } else {
                        "."
                    };
                    ui.monospace(format!("{}:{}{}", name, ie, ifr));
                }
            });
            ui.separator();

            ui.heading("PPU");
            ui.monospace(format!(
                "LY:{:02X}({:3})  LCDC:{:02X}  STAT:{:02X}",
                info.ppu.ly, info.ppu.ly, info.ppu.lcdc, info.ppu.stat
            ));
            let mode = info.ppu.stat & 0x03;
            let mode_name = match mode {
                0 => "HBlank",
                1 => "VBlank",
                2 => "OAM Scan",
                3 => "Drawing",
                _ => "?",
            };
            ui.monospace(format!("Mode: {} ({})", mode, mode_name));
            ui.separator();

            ui.heading("Timer");
            ui.monospace(format!(
                "DIV:{:02X}  TIMA:{:02X}  TMA:{:02X}  TAC:{:02X}",
                info.div, info.tima, info.tma, info.tac
            ));
            let enabled = if info.tac & 0x04 != 0 { "ON" } else { "OFF" };
            let clock = match info.tac & 0x03 {
                0 => "4096 Hz",
                1 => "262144 Hz",
                2 => "65536 Hz",
                3 => "16384 Hz",
                _ => "?",
            };
            ui.monospace(format!("Timer: {} @ {}", enabled, clock));
            ui.separator();

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
            ui.separator();

            ui.heading("Recent Opcodes");
            for &(pc, op, is_cb) in &info.recent_ops {
                if is_cb {
                    ui.monospace(format!("{:04X}: CB {:02X}", pc, op));
                } else {
                    ui.monospace(format!("{:04X}: {:02X}", pc, op));
                }
            }

            let suspended = info.cpu_state == "Suspended";
            if suspended {
                ui.separator();
                let button = egui::Button::new("▶ Continue (F5)")
                    .fill(egui::Color32::from_rgb(40, 100, 40));
                if ui.add(button).clicked() {
                    actions.continue_requested = true;
                }
            }

            ui.horizontal(|ui| {
                if ui.button("Step").clicked() {
                    actions.step_requested = true;
                }
                if !suspended {
                    if ui.button("Continue").clicked() {
                        actions.continue_requested = true;
                    }
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
