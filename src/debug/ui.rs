use crate::debug::DebugInfo;
use crate::debug::DebugWindowState;
use crate::debug::breakpoints::WatchType;
use crate::debug::dock::{DebugTab, toggle_dock_tab};
use crate::graphics::AspectRatioMode;
use crate::hardware::types::hardware_mode::HardwareModePreference;
use crate::settings::{
    BindingAction, InputBindingAction, LeftStickMode, Settings, TiltBindingAction, TiltInputMode,
};
use egui_dock::DockState;

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

    pub(crate) fn has_pending(&self) -> bool {
        self.add_breakpoint.is_some()
            || self.add_watchpoint.is_some()
            || !self.remove_breakpoints.is_empty()
            || !self.toggle_breakpoints.is_empty()
            || !self.memory_writes.is_empty()
            || self.apu_channel_mutes.is_some()
    }
}

pub(crate) struct MenuActions {
    pub(crate) open_file_requested: bool,
    pub(crate) open_settings_requested: bool,
    pub(crate) save_state_file_requested: bool,
    pub(crate) load_state_file_requested: bool,
    pub(crate) save_state_slot: Option<u8>,
    pub(crate) load_state_slot: Option<u8>,
    pub(crate) aspect_ratio_mode: Option<AspectRatioMode>,
    pub(crate) menu_bar_height_points: f32,
}

pub(crate) fn draw_menu_bar(
    ctx: &egui::Context,
    current_mode: AspectRatioMode,
    dock_state: &mut DockState<DebugTab>,
) -> MenuActions {
    let mut open_file_requested = false;
    let mut open_settings_requested = false;
    let mut save_state_file_requested = false;
    let mut load_state_file_requested = false;
    let mut save_state_slot = None;
    let mut load_state_slot = None;
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
                ui.separator();
                if ui.button("Save State (File...)...").clicked() {
                    save_state_file_requested = true;
                    ui.close();
                }
                if ui.button("Load State (File...)...").clicked() {
                    load_state_file_requested = true;
                    ui.close();
                }
                ui.separator();
                for slot in 1..=4u8 {
                    if ui.button(format!("Save State (Slot {slot})")).clicked() {
                        save_state_slot = Some(slot);
                        ui.close();
                    }
                }
                ui.separator();
                for slot in 1..=4u8 {
                    if ui.button(format!("Load State (Slot {slot})")).clicked() {
                        load_state_slot = Some(slot);
                        ui.close();
                    }
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
                    toggle_dock_tab(dock_state, DebugTab::CpuDebug);
                    ui.close();
                }
                if ui.button("APU / Sound").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::ApuViewer);
                    ui.close();
                }
                if ui.button("ROM Info").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::RomInfo);
                    ui.close();
                }
                if ui.button("Disassembler").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::Disassembler);
                    ui.close();
                }
                if ui.button("Memory Viewer").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::MemoryViewer);
                    ui.close();
                }
                if ui.button("Tile Data").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::TileViewer);
                    ui.close();
                }
                if ui.button("Tile Map").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::TilemapViewer);
                    ui.close();
                }
                if ui.button("OAM / Sprites").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::OamViewer);
                    ui.close();
                }
                if ui.button("Palettes").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::PaletteViewer);
                    ui.close();
                }
            });
        });
    });

    MenuActions {
        open_file_requested,
        open_settings_requested,
        save_state_file_requested,
        load_state_file_requested,
        save_state_slot,
        load_state_slot,
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
                    .text("Fast-forward multiplier"),
            );
            ui.add(
                egui::Slider::new(&mut settings.uncapped_frames_per_tick, 1..=240)
                    .text("Uncapped frames/tick"),
            );
            ui.checkbox(&mut settings.uncapped_speed, "Start in uncapped mode");
            ui.checkbox(&mut settings.frame_skip, "Frame skip when behind")
                .on_hover_text(
                    "When enabled, skip emulation frames to stay in real-time if the \
                     host can't keep up. When disabled, the emulator catches up \
                     gradually (more accurate, may drift behind).",
                );

            ui.separator();
            ui.heading("Input");
            if let Some(action) = state.rebinding_action {
                let label = match action {
                    InputBindingAction::Joypad(a) => joypad_binding_label(a),
                    InputBindingAction::Tilt(a) => tilt_binding_label(a),
                };
                ui.label(format!("Press a key for {}...", label));
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
                    ui.label(joypad_binding_label(action));
                    let key_name = format!("{:?}", settings.key_bindings.get(action));
                    let capture_label =
                        if state.rebinding_action == Some(InputBindingAction::Joypad(action)) {
                            format!("Press key... ({key_name})")
                        } else {
                            key_name
                        };
                    if ui.button(capture_label).clicked() {
                        state.rebinding_action = Some(InputBindingAction::Joypad(action));
                    }
                });
            }

            ui.separator();
            ui.heading("MBC7 Tilt");
            egui::ComboBox::from_label("Left stick behavior")
                .selected_text(match settings.left_stick_mode {
                    LeftStickMode::Auto => "Auto (Tilt on MBC7, D-pad otherwise)",
                    LeftStickMode::Tilt => "Always Tilt",
                    LeftStickMode::Dpad => "Always D-pad",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.left_stick_mode,
                        LeftStickMode::Auto,
                        "Auto (Tilt on MBC7, D-pad otherwise)",
                    );
                    ui.selectable_value(
                        &mut settings.left_stick_mode,
                        LeftStickMode::Tilt,
                        "Always Tilt",
                    );
                    ui.selectable_value(
                        &mut settings.left_stick_mode,
                        LeftStickMode::Dpad,
                        "Always D-pad",
                    );
                });
            egui::ComboBox::from_label("Tilt input source")
                .selected_text(match settings.tilt_input_mode {
                    TiltInputMode::Keyboard => "Keyboard (WASD)",
                    TiltInputMode::Mouse => "Mouse",
                    TiltInputMode::Auto => "Auto-detect",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.tilt_input_mode,
                        TiltInputMode::Keyboard,
                        "Keyboard (WASD)",
                    );
                    ui.selectable_value(
                        &mut settings.tilt_input_mode,
                        TiltInputMode::Mouse,
                        "Mouse",
                    );
                    ui.selectable_value(
                        &mut settings.tilt_input_mode,
                        TiltInputMode::Auto,
                        "Auto-detect",
                    );
                });
            ui.checkbox(&mut settings.tilt_invert_x, "Invert tilt X");
            ui.checkbox(&mut settings.tilt_invert_y, "Invert tilt Y");
            ui.checkbox(
                &mut settings.stick_tilt_bypass_lerp,
                "Use direct left-stick tilt (bypass lerp while stick is active)",
            );
            ui.add(
                egui::Slider::new(&mut settings.tilt_sensitivity, 0.1..=3.0)
                    .text("Tilt sensitivity"),
            );
            ui.add(egui::Slider::new(&mut settings.tilt_lerp, 0.0..=1.0).text("Tilt smoothing"));
            ui.add(egui::Slider::new(&mut settings.tilt_deadzone, 0.0..=0.5).text("Tilt deadzone"));
            if ui.button("Reset tilt keys to WASD").clicked() {
                settings.tilt_key_bindings.set_wasd_defaults();
            }
            for action in [
                TiltBindingAction::Up,
                TiltBindingAction::Down,
                TiltBindingAction::Left,
                TiltBindingAction::Right,
            ] {
                ui.horizontal(|ui| {
                    ui.label(tilt_binding_label(action));
                    let key_name = format!("{:?}", settings.tilt_key_bindings.get(action));
                    let capture_label =
                        if state.rebinding_action == Some(InputBindingAction::Tilt(action)) {
                            format!("Press key... ({key_name})")
                        } else {
                            key_name
                        };
                    if ui.button(capture_label).clicked() {
                        state.rebinding_action = Some(InputBindingAction::Tilt(action));
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

fn joypad_binding_label(action: BindingAction) -> &'static str {
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

fn tilt_binding_label(action: TiltBindingAction) -> &'static str {
    match action {
        TiltBindingAction::Up => "Tilt Up",
        TiltBindingAction::Down => "Tilt Down",
        TiltBindingAction::Left => "Tilt Left",
        TiltBindingAction::Right => "Tilt Right",
    }
}


pub(super) fn draw_debug_ui_content(
    ui: &mut egui::Ui,
    info: &DebugInfo,
    window_state: &mut DebugWindowState,
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
            ui.collapsing("Tilt Input", |ui| {
                ui.monospace(format!(
                    "MBC7 active: {}",
                    if info.tilt_is_mbc7 { "yes" } else { "no" }
                ));
                ui.monospace(format!(
                    "Left stick routes to: {}",
                    if info.tilt_stick_controls_tilt {
                        "tilt"
                    } else {
                        "d-pad"
                    }
                ));
                ui.monospace(format!(
                    "Keyboard  x:{:>6.2} y:{:>6.2}",
                    info.tilt_keyboard.0, info.tilt_keyboard.1
                ));
                ui.monospace(format!(
                    "Mouse     x:{:>6.2} y:{:>6.2}",
                    info.tilt_mouse.0, info.tilt_mouse.1
                ));
                ui.monospace(format!(
                    "LeftStick x:{:>6.2} y:{:>6.2}",
                    info.tilt_left_stick.0, info.tilt_left_stick.1
                ));
                ui.separator();
                ui.monospace(format!(
                    "Target    x:{:>6.2} y:{:>6.2}",
                    info.tilt_target.0, info.tilt_target.1
                ));
                ui.monospace(format!(
                    "Smoothed  x:{:>6.2} y:{:>6.2}",
                    info.tilt_smoothed.0, info.tilt_smoothed.1
                ));

                let smoothed_x = ((info.tilt_smoothed.0 + 1.0) * 0.5).clamp(0.0, 1.0);
                let smoothed_y = ((info.tilt_smoothed.1 + 1.0) * 0.5).clamp(0.0, 1.0);
                ui.add(
                    egui::ProgressBar::new(smoothed_x)
                        .show_percentage()
                        .text("Smoothed X (-1..1)"),
                );
                ui.add(
                    egui::ProgressBar::new(smoothed_y)
                        .show_percentage()
                        .text("Smoothed Y (-1..1)"),
                );
            });
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
}
