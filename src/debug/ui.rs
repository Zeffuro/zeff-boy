use crate::debug::DebugInfo;
use crate::debug::DebugWindowState;
use crate::graphics::AspectRatioMode;

pub(crate) struct MenuActions {
    pub(crate) open_file_requested: bool,
    pub(crate) aspect_ratio_mode: Option<AspectRatioMode>,
    pub(crate) menu_bar_height_points: f32,
}

pub(crate) fn draw_menu_bar(
    ctx: &egui::Context,
    current_mode: AspectRatioMode,
    debug_windows: &mut DebugWindowState,
) -> MenuActions {
    let mut open_file_requested = false;
    let mut selected_mode = None;

    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open").clicked() {
                    open_file_requested = true;
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                if ui
                    .selectable_label(current_mode == AspectRatioMode::Stretch, "Stretch")
                    .clicked()
                {
                    selected_mode = Some(AspectRatioMode::Stretch);
                    ui.close();
                }
                if ui
                    .selectable_label(current_mode == AspectRatioMode::KeepAspect, "Keep Aspect")
                    .clicked()
                {
                    selected_mode = Some(AspectRatioMode::KeepAspect);
                    ui.close();
                }
                if ui
                    .selectable_label(current_mode == AspectRatioMode::IntegerScale, "Integer Scale")
                    .clicked()
                {
                    selected_mode = Some(AspectRatioMode::IntegerScale);
                    ui.close();
                }
            });

            ui.menu_button("Debug", |ui| {
                if ui.button("Tile Data").clicked() {
                    debug_windows.show_tile_viewer = !debug_windows.show_tile_viewer;
                    ui.close();
                }
                if ui.button("Tile Map").clicked() {
                    debug_windows.show_tilemap_viewer = !debug_windows.show_tilemap_viewer;
                    ui.close();
                }
                if ui.button("OAM / Sprites").clicked() {
                    debug_windows.show_oam_viewer = !debug_windows.show_oam_viewer;
                    ui.close();
                }
                if ui.button("Palettes").clicked() {
                    debug_windows.show_palette_viewer = !debug_windows.show_palette_viewer;
                    ui.close();
                }
            });
        });
    });

    MenuActions {
        open_file_requested,
        aspect_ratio_mode: selected_mode,
        menu_bar_height_points: ctx.available_rect().min.y.max(0.0),
    }
}

pub(crate) fn draw_debug_ui(ctx: &egui::Context, info: &DebugInfo) {
    egui::Window::new("CPU / Debug")
        .default_pos([10.0, 10.0])
        .default_width(260.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("Performance");
            ui.label(format!("FPS: {:.1}", info.fps));
            ui.label(format!("Total cycles: {}", info.cycles));
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
            ui.monospace(format!("@ {:04X} = {:02X}", info.last_opcode_pc, info.last_opcode));
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
                    let ifr = if info.if_reg & (1 << i) != 0 { "F" } else { "." };
                    ui.monospace(format!("{}:{}{}", name, ie, ifr));
                }
            });
            ui.separator();

            ui.heading("PPU");
            ui.monospace(format!(
                "LY:{:02X}({:3})  LCDC:{:02X}  STAT:{:02X}",
                info.ly, info.ly, info.lcdc, info.stat
            ));
            let mode = info.stat & 0x03;
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
            for entry in &info.recent_ops {
                ui.monospace(entry);
            }
        });
}

