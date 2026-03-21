use crate::debug::DebugInfo;
use crate::debug::DebugWindowState;
use crate::debug::breakpoints::WatchType;
use crate::graphics::AspectRatioMode;
use crate::hardware::types::hardware_mode::HardwareModePreference;
use crate::settings::{BindingAction, Settings};

pub(crate) struct DebugUiActions {
    pub(crate) add_breakpoint: Option<u16>,
    pub(crate) add_watchpoint: Option<(u16, WatchType)>,
    pub(crate) remove_breakpoints: Vec<u16>,
    pub(crate) toggle_breakpoints: Vec<u16>,
    pub(crate) memory_writes: Vec<(u16, u8)>,
    pub(crate) apu_channel_mutes: Option<[bool; 4]>,
    pub(crate) step_requested: bool,
    pub(crate) continue_requested: bool,
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
        }
    }
}

pub(crate) struct MenuActions {
    pub(crate) open_file_requested: bool,
    pub(crate) open_settings_requested: bool,
    pub(crate) aspect_ratio_mode: Option<AspectRatioMode>,
    pub(crate) menu_bar_height_points: f32,
}

pub(crate) fn draw_menu_bar(
    ctx: &egui::Context,
    current_mode: AspectRatioMode,
    debug_windows: &mut DebugWindowState,
) -> MenuActions {
    let mut open_file_requested = false;
    let mut open_settings_requested = false;
    let mut selected_mode = None;

    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open").clicked() {
                    open_file_requested = true;
                    ui.close();
                }
                if ui.button("Settings").clicked() {
                    open_settings_requested = true;
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
                    .selectable_label(
                        current_mode == AspectRatioMode::IntegerScale,
                        "Integer Scale",
                    )
                    .clicked()
                {
                    selected_mode = Some(AspectRatioMode::IntegerScale);
                    ui.close();
                }
            });

            ui.menu_button("Debug", |ui| {
                if ui.button("CPU / Debug").clicked() {
                    debug_windows.show_cpu_debug = !debug_windows.show_cpu_debug;
                    ui.close();
                }
                if ui.button("APU / Sound").clicked() {
                    debug_windows.show_apu_viewer = !debug_windows.show_apu_viewer;
                    ui.close();
                }
                if ui.button("ROM Info").clicked() {
                    debug_windows.show_rom_info = !debug_windows.show_rom_info;
                    ui.close();
                }
                if ui.button("Disassembler").clicked() {
                    debug_windows.show_disassembler = !debug_windows.show_disassembler;
                    ui.close();
                }
                if ui.button("Memory Viewer").clicked() {
                    debug_windows.show_memory_viewer = !debug_windows.show_memory_viewer;
                    ui.close();
                }
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
        open_settings_requested,
        aspect_ratio_mode: selected_mode,
        menu_bar_height_points: ctx.available_rect().min.y.max(0.0),
    }
}

pub(crate) fn draw_settings_window(
    ctx: &egui::Context,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    open: &mut bool,
) {
    egui::Window::new("Settings")
        .open(open)
        .default_width(360.0)
        .show(ctx, |ui| {
            ui.heading("Emulation");
            egui::ComboBox::from_label("Hardware mode")
                .selected_text(match settings.hardware_mode_preference {
                    HardwareModePreference::Auto => "Auto",
                    HardwareModePreference::ForceDmg => "DMG",
                    HardwareModePreference::ForceCgb => "CGB",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.hardware_mode_preference,
                        HardwareModePreference::Auto,
                        "Auto",
                    );
                    ui.selectable_value(
                        &mut settings.hardware_mode_preference,
                        HardwareModePreference::ForceDmg,
                        "DMG",
                    );
                    ui.selectable_value(
                        &mut settings.hardware_mode_preference,
                        HardwareModePreference::ForceCgb,
                        "CGB",
                    );
                });

            ui.add(
                egui::Slider::new(&mut settings.fast_forward_multiplier, 1..=16)
                    .text("Fast-forward frames/tick"),
            );
            ui.add(
                egui::Slider::new(&mut settings.uncapped_frames_per_tick, 1..=240)
                    .text("Uncapped frames/tick"),
            );
            ui.checkbox(&mut settings.uncapped_speed, "Start in uncapped mode");

            ui.separator();
            ui.heading("Input");
            if let Some(action) = state.rebinding_action {
                ui.label(format!("Press a key for {}...", binding_label(action)));
            }
            for action in [
                BindingAction::Up,
                BindingAction::Down,
                BindingAction::Left,
                BindingAction::Right,
                BindingAction::A,
                BindingAction::B,
                BindingAction::Start,
                BindingAction::Select,
            ] {
                ui.horizontal(|ui| {
                    ui.label(binding_label(action));
                    let key_name = format!("{:?}", settings.key_bindings.get(action));
                    let capture_label = if state.rebinding_action == Some(action) {
                        format!("Press key... ({key_name})")
                    } else {
                        key_name
                    };
                    if ui.button(capture_label).clicked() {
                        state.rebinding_action = Some(action);
                    }
                });
            }

            ui.separator();
            ui.heading("Audio");
            ui.add(
                egui::Slider::new(&mut settings.master_volume, 0.0..=1.0)
                    .text("Master volume")
                    .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
            );
            ui.checkbox(
                &mut settings.mute_audio_during_fast_forward,
                "Mute audio while fast-forward is held",
            );

            ui.separator();
            ui.heading("UI");
            ui.checkbox(&mut settings.show_fps, "Show FPS in debug panel");

            ui.separator();
            if ui.button("Reset to defaults").clicked() {
                *settings = Settings::default();
                state.rebinding_action = None;
            }
        });
}

fn binding_label(action: BindingAction) -> &'static str {
    match action {
        BindingAction::Up => "Up",
        BindingAction::Down => "Down",
        BindingAction::Left => "Left",
        BindingAction::Right => "Right",
        BindingAction::A => "A",
        BindingAction::B => "B",
        BindingAction::Start => "Start",
        BindingAction::Select => "Select",
    }
}

pub(crate) fn draw_debug_ui(
    ctx: &egui::Context,
    info: &DebugInfo,
    window_state: &mut DebugWindowState,
) -> DebugUiActions {
    let mut actions = DebugUiActions::none();
    egui::Window::new("CPU / Debug")
        .open(&mut window_state.show_cpu_debug)
        .default_pos([10.0, 10.0])
        .default_width(260.0)
        .resizable(true)
        .show(ctx, |ui| {
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
            for entry in &info.recent_ops {
                ui.monospace(entry);
            }

            ui.separator();
            ui.heading("Breakpoints");
            ui.horizontal(|ui| {
                ui.label("Addr:");
                ui.text_edit_singleline(&mut window_state.breakpoint_input);
                if ui.button("Add BP").clicked() {
                    let trimmed = window_state.breakpoint_input.trim();
                    let parsed = if let Some(hex) = trimmed
                        .strip_prefix("0x")
                        .or_else(|| trimmed.strip_prefix("0X"))
                    {
                        u16::from_str_radix(hex, 16).ok()
                    } else {
                        u16::from_str_radix(trimmed, 16)
                            .ok()
                            .or_else(|| trimmed.parse().ok())
                    };
                    if let Some(addr) = parsed {
                        actions.add_breakpoint = Some(addr);
                        window_state.breakpoint_input.clear();
                    }
                }
            });

            if info.breakpoints.is_empty() {
                ui.monospace("(none)");
            } else {
                let mut remove = None;
                for bp in &info.breakpoints {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{:04X}", bp));
                        if ui.button("Remove").clicked() {
                            remove = Some(*bp);
                        }
                    });
                }
                if let Some(addr) = remove {
                    actions.remove_breakpoints.push(addr);
                }
            }

            ui.separator();
            ui.heading("Watchpoints");
            ui.horizontal(|ui| {
                ui.label("Addr:");
                ui.text_edit_singleline(&mut window_state.watchpoint_input);
                egui::ComboBox::from_id_salt("watch_type")
                    .selected_text(match window_state.watchpoint_type {
                        WatchType::Read => "Read",
                        WatchType::Write => "Write",
                        WatchType::ReadWrite => "Read/Write",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut window_state.watchpoint_type,
                            WatchType::Read,
                            "Read",
                        );
                        ui.selectable_value(
                            &mut window_state.watchpoint_type,
                            WatchType::Write,
                            "Write",
                        );
                        ui.selectable_value(
                            &mut window_state.watchpoint_type,
                            WatchType::ReadWrite,
                            "Read/Write",
                        );
                    });
                if ui.button("Add WP").clicked() {
                    let trimmed = window_state.watchpoint_input.trim();
                    let parsed = if let Some(hex) = trimmed
                        .strip_prefix("0x")
                        .or_else(|| trimmed.strip_prefix("0X"))
                    {
                        u16::from_str_radix(hex, 16).ok()
                    } else {
                        u16::from_str_radix(trimmed, 16)
                            .ok()
                            .or_else(|| trimmed.parse().ok())
                    };
                    if let Some(addr) = parsed {
                        actions.add_watchpoint = Some((addr, window_state.watchpoint_type));
                        window_state.watchpoint_input.clear();
                    }
                }
            });

            if info.watchpoints.is_empty() {
                ui.monospace("(none)");
            } else {
                for wp in &info.watchpoints {
                    ui.monospace(wp);
                }
            }

            ui.horizontal(|ui| {
                if ui.button("Step").clicked() {
                    actions.step_requested = true;
                }
                if ui.button("Continue").clicked() {
                    actions.continue_requested = true;
                }
            });

            if let Some(bp) = info.hit_breakpoint {
                ui.monospace(format!("Hit breakpoint @ {:04X}", bp));
            }
            if let Some(hit) = &info.hit_watchpoint {
                ui.monospace(format!("Watch hit: {}", hit));
            }
        });

    actions
}
