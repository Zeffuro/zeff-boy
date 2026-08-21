use super::enums::{ShaderParams, ShaderPreset};
use super::structs::{default_rewind_seconds, default_rewind_speed};
use super::tilt_bindings::TiltKeyBindings;
use super::*;

#[test]
fn settings_default_roundtrip() {
    let defaults = Settings::default();
    assert_eq!(defaults.video.pce_overscan_mode, PceOverscanMode::Full);
    assert_eq!(defaults.video.pce_palette_mode, PcePaletteMode::RawRgb);
    let json = serde_json::to_string_pretty(&defaults).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(defaults, restored);
}

#[test]
fn settings_with_modified_values_roundtrip() {
    let mut s = Settings::default();
    s.emulation.fast_forward_multiplier = 8;
    s.emulation.slow_motion_divisor = 6;
    s.emulation.slow_motion_enabled = true;
    s.audio.volume = 0.5;
    s.rewind.speed = 5;
    s.rewind.seconds = 30;
    s.rewind.enabled = false;
    s.video.shader_preset = ShaderPreset::Crt;
    s.video.custom_shader_path = "C:/shaders/custom.wgsl".to_string();
    s.ui.autohide_menu_bar = true;
    s.emulation.frame_skip = true;

    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(s, restored);
}

#[test]
fn settings_backward_compat_missing_fields_use_defaults() {
    let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.rewind.speed, default_rewind_speed());
    assert_eq!(s.rewind.seconds, default_rewind_seconds());
    assert_eq!(s.video.shader_preset, ShaderPreset::None);
    assert!(!s.ui.autohide_menu_bar);
    assert_eq!(s.ui.debug_presentation, DebugPresentation::default());
    assert_eq!(s.ui.ui_density, UiDensity::default());
    assert!(s.ui.debugger_window_open);
    assert_eq!(s.ui.debugger_window_size, [1100, 760]);
    assert_eq!(s.ui.settings_window_size, [760, 680]);
    assert_eq!(s.ui.printer_window_size, [520, 720]);
    assert_eq!(s.emulation.slow_motion_divisor, 4);
    assert!(!s.emulation.slow_motion_enabled);
    assert_eq!(
        s.emulation.sega8_video_standard,
        Sega8VideoStandardPreference::Auto
    );
    assert_eq!(
        s.emulation.sega8_console_region,
        Sega8ConsoleRegionPreference::Auto
    );
    assert_eq!(s.key_bindings_p2.up, KeyCode::Numpad8);
    assert_eq!(s.key_bindings.l, KeyCode::KeyA);
    assert_eq!(s.key_bindings.r, KeyCode::KeyS);
    assert_eq!(s.key_bindings_p2.l, KeyCode::Numpad7);
    assert_eq!(s.key_bindings_p2.r, KeyCode::Numpad9);
    assert_eq!(s.gamepad_bindings.get_p2(BindingAction::A), "South");
    assert_eq!(s.gamepad_bindings.get(BindingAction::L), "LeftTrigger");
    assert_eq!(s.gamepad_bindings.get(BindingAction::R), "RightTrigger");
    assert_eq!(s.emulation.firmware_directory, "");
    assert_eq!(s.emulation.firmware_directory_path(), None);
    assert_eq!(s.emulation.gba_bios_mode, GbaBiosMode::Hle);
    assert_eq!(s.emulation.gb_boot_rom_mode, GbBootRomMode::Skip);
    assert_eq!(s.emulation.sega_boot_rom_mode, SegaBootRomMode::Skip);
}

#[test]
fn debugger_palette_tracks_theme_until_customized() {
    let mut ui = UiSettings {
        theme_preset: UiThemePreset::Light,
        ..UiSettings::default()
    };
    assert_eq!(
        ui.effective_debug_colors(),
        DebugColors::for_theme(UiThemePreset::Light)
    );

    ui.debug_colors.address = [1, 2, 3, 255];
    assert_eq!(ui.effective_debug_colors().address, [1, 2, 3, 255]);
}

#[test]
fn firmware_directory_setting_roundtrip_and_trims_empty_path() {
    let mut s = Settings::default();
    s.emulation.firmware_directory = "  ".to_string();
    assert_eq!(s.emulation.firmware_directory_path(), None);

    s.emulation.firmware_directory = "F:/Firmware".to_string();
    s.emulation.gba_bios_mode = GbaBiosMode::External;
    s.emulation.gb_boot_rom_mode = GbBootRomMode::External;
    s.emulation.sega_boot_rom_mode = SegaBootRomMode::External;
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.emulation.firmware_directory, "F:/Firmware");
    assert_eq!(restored.emulation.gba_bios_mode, GbaBiosMode::External);
    assert_eq!(restored.emulation.gb_boot_rom_mode, GbBootRomMode::External);
    assert_eq!(
        restored.emulation.sega_boot_rom_mode,
        SegaBootRomMode::External
    );
    assert_eq!(
        restored.emulation.firmware_directory_path(),
        Some(PathBuf::from("F:/Firmware"))
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn firmware_search_dirs_use_configured_dir_before_legacy_env_fallback() {
    let mut s = Settings::default();
    s.emulation.firmware_directory = "F:/Firmware".to_string();

    let dirs = s.emulation.firmware_search_dirs_with_roots(
        PathBuf::from("F:/ManagedFirmware"),
        Some(PathBuf::from("F:/EnvFirmware")),
    );

    assert_eq!(
        dirs,
        vec![
            PathBuf::from("F:/ManagedFirmware"),
            PathBuf::from("F:/Firmware"),
            PathBuf::from("F:/EnvFirmware")
        ]
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn firmware_search_dirs_deduplicate_the_managed_root() {
    let mut s = Settings::default();
    s.emulation.firmware_directory = "F:/ManagedFirmware".to_string();

    assert_eq!(
        s.emulation.firmware_search_dirs_with_roots(
            PathBuf::from("F:/ManagedFirmware"),
            Some(PathBuf::from("F:/ManagedFirmware")),
        ),
        vec![PathBuf::from("F:/ManagedFirmware")]
    );
}

#[test]
fn sega8_video_standard_preference_serde_roundtrip() {
    let mut s = Settings::default();
    s.emulation.sega8_video_standard = Sega8VideoStandardPreference::Pal;

    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();

    assert_eq!(
        restored.emulation.sega8_video_standard,
        Sega8VideoStandardPreference::Pal
    );
}

#[test]
fn sega8_console_region_preference_serde_roundtrip() {
    let mut s = Settings::default();
    s.emulation.sega8_console_region = Sega8ConsoleRegionPreference::JapanesePowerBaseConverter;

    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();

    assert_eq!(
        restored.emulation.sega8_console_region,
        Sega8ConsoleRegionPreference::JapanesePowerBaseConverter
    );
}

#[test]
fn pce_console_wiring_preference_serde_roundtrip() {
    let mut settings = Settings::default();
    settings.emulation.pce_console_wiring = PceConsoleWiringPreference::TurboGrafx16;
    settings.emulation.pce_cd_archive_memory_limit = PceCdArchiveMemoryLimit::MiB256;

    let json = serde_json::to_string(&settings).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();

    assert_eq!(
        restored.emulation.pce_console_wiring,
        PceConsoleWiringPreference::TurboGrafx16
    );
    assert_eq!(
        restored.emulation.pce_console_wiring.forced_wiring(),
        Some(zeff_pce_core::hardware::PceConsoleWiring::TurboGrafx16)
    );
    assert_eq!(
        restored.emulation.pce_cd_archive_memory_limit,
        PceCdArchiveMemoryLimit::MiB256
    );
    assert_eq!(restored.emulation.pce_cd_archive_memory_limit.mib(), 256);
}

#[test]
fn pce_controller_settings_serde_roundtrip() {
    let mut settings = Settings::default();
    settings.emulation.pce_controller = PceControllerPreference::Mouse;
    settings.emulation.pce_mouse_sensitivity = 2.5;
    settings.emulation.pce_mouse_cursor_mode = PceMouseCursorMode::Captured;

    let encoded = serde_json::to_string(&settings).unwrap();
    let restored: Settings = serde_json::from_str(&encoded).unwrap();

    assert_eq!(
        restored.emulation.pce_controller,
        PceControllerPreference::Mouse
    );
    assert_eq!(restored.emulation.pce_mouse_sensitivity, 2.5);
    assert_eq!(
        restored.emulation.pce_mouse_cursor_mode,
        PceMouseCursorMode::Captured
    );
}

#[test]
fn pce_multitap_setting_serde_roundtrip() {
    let mut settings = Settings::default();
    settings.emulation.pce_controller = PceControllerPreference::Multitap;

    let encoded = serde_json::to_string(&settings).unwrap();
    let restored: Settings = serde_json::from_str(&encoded).unwrap();

    assert_eq!(
        restored.emulation.pce_controller,
        PceControllerPreference::Multitap
    );
}

#[test]
fn key_bindings_serde_roundtrip() {
    let bindings = KeyBindings {
        a: KeyCode::KeyQ,
        b: KeyCode::KeyE,
        ..KeyBindings::default()
    };

    let json = serde_json::to_string(&bindings).unwrap();
    let restored: KeyBindings = serde_json::from_str(&json).unwrap();
    assert_eq!(bindings, restored);
}

#[test]
fn key_bindings_deserialize_unknown_falls_back_to_defaults() {
    let json = r#"{"up":"ArrowUp","down":"ArrowDown","left":"UNKNOWN_KEY","right":"ArrowRight","a":"KeyZ","b":"KeyX","start":"Enter","select":"ShiftRight"}"#;
    let bindings: KeyBindings = serde_json::from_str(json).unwrap();
    assert_eq!(bindings.left, KeyCode::ArrowLeft);
    assert_eq!(bindings.up, KeyCode::ArrowUp);
    assert_eq!(bindings.l, KeyCode::KeyA);
    assert_eq!(bindings.r, KeyCode::KeyS);
}

#[test]
fn player_two_key_bindings_use_numpad_defaults() {
    let bindings = KeyBindings::player_two_defaults();
    assert_eq!(bindings.up, KeyCode::Numpad8);
    assert_eq!(bindings.down, KeyCode::Numpad5);
    assert_eq!(bindings.left, KeyCode::Numpad4);
    assert_eq!(bindings.right, KeyCode::Numpad6);
    assert_eq!(bindings.a, KeyCode::Numpad1);
    assert_eq!(bindings.b, KeyCode::Numpad2);
    assert_eq!(bindings.l, KeyCode::Numpad7);
    assert_eq!(bindings.r, KeyCode::Numpad9);
    assert_eq!(bindings.start, KeyCode::NumpadEnter);
    assert_eq!(bindings.select, KeyCode::Numpad0);
}

#[test]
fn wonderswan_key_bindings_serde_roundtrip() {
    let bindings = WonderSwanKeyBindings {
        x1: KeyCode::KeyI,
        y4: KeyCode::KeyJ,
        ..WonderSwanKeyBindings::default()
    };

    let json = serde_json::to_string(&bindings).unwrap();
    let restored: WonderSwanKeyBindings = serde_json::from_str(&json).unwrap();
    assert_eq!(bindings, restored);
}

#[test]
fn wonderswan_key_bindings_deserialize_unknown_falls_back_to_defaults() {
    let json = r#"{"x1":"UNKNOWN_KEY","x2":"KeyD","x3":"KeyS","x4":"KeyA","y1":"ArrowUp","y2":"ArrowRight","y3":"ArrowDown","y4":"ArrowLeft","a":"KeyX","b":"KeyZ","start":"Enter"}"#;
    let bindings: WonderSwanKeyBindings = serde_json::from_str(json).unwrap();
    assert_eq!(bindings.x1, KeyCode::KeyW);
    assert_eq!(bindings.y4, KeyCode::ArrowLeft);
}

#[test]
fn shortcut_bindings_get_returns_default_for_unknown_string() {
    let bindings = ShortcutBindings {
        fullscreen: "NONSENSE".to_string(),
        ..ShortcutBindings::default()
    };

    assert_eq!(bindings.get(ShortcutAction::Fullscreen), KeyCode::F11);
}

#[test]
fn shortcut_bindings_default_slow_motion_uses_f4() {
    assert_eq!(
        ShortcutBindings::default().get(ShortcutAction::SlowMotion),
        KeyCode::F4
    );
}

#[test]
fn shortcut_bindings_set_and_get() {
    let mut bindings = ShortcutBindings::default();
    bindings.set(ShortcutAction::Pause, KeyCode::KeyP);
    assert_eq!(bindings.get(ShortcutAction::Pause), KeyCode::KeyP);
}

#[test]
fn gamepad_bindings_roundtrip() {
    let mut gb = GamepadBindings::default();
    gb.set(BindingAction::A, "West");
    gb.set(BindingAction::R, "RightTrigger2");
    gb.set_p2(BindingAction::B, "North");
    gb.set_p2(BindingAction::L, "LeftTrigger2");
    gb.set_ws(WonderSwanButton::Y4, "RightTrigger");
    let json = serde_json::to_string(&gb).unwrap();
    let restored: GamepadBindings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.get(BindingAction::A), "West");
    assert_eq!(restored.get(BindingAction::B), "East");
    assert_eq!(restored.get(BindingAction::L), "LeftTrigger");
    assert_eq!(restored.get(BindingAction::R), "RightTrigger2");
    assert_eq!(restored.get_p2(BindingAction::A), "South");
    assert_eq!(restored.get_p2(BindingAction::B), "North");
    assert_eq!(restored.get_p2(BindingAction::L), "LeftTrigger2");
    assert_eq!(restored.get_p2(BindingAction::R), "RightTrigger");
    assert!(matches!(
        restored.map_button_name_p2("North"),
        Some(crate::input::HostButton::B)
    ));
    assert!(matches!(
        restored.map_button_name("RightTrigger2"),
        Some(crate::input::HostButton::R)
    ));
    assert!(matches!(
        restored.map_button_name_p2("LeftTrigger2"),
        Some(crate::input::HostButton::L)
    ));
    assert_eq!(restored.get_ws(WonderSwanButton::A), "South");
    assert_eq!(restored.get_ws(WonderSwanButton::B), "East");
    assert_eq!(restored.get_ws(WonderSwanButton::Start), "Start");
    assert_eq!(restored.get_ws(WonderSwanButton::Y4), "RightTrigger");
    assert_eq!(
        restored.map_ws_button_name("RightTrigger"),
        Some(WonderSwanButton::Y4)
    );
    assert_eq!(
        restored.map_ws_button_name("South"),
        Some(WonderSwanButton::A)
    );
}

#[test]
fn gamepad_bindings_deserialize_missing_ws_buttons_to_defaults() {
    let json = r#"{
        "a":"South",
        "b":"East",
        "start":"Start",
        "select":"Select",
        "up":"DPadUp",
        "down":"DPadDown",
        "left":"DPadLeft",
        "right":"DPadRight"
    }"#;
    let restored: GamepadBindings = serde_json::from_str(json).unwrap();
    assert_eq!(restored.get_p2(BindingAction::A), "South");
    assert_eq!(restored.get_p2(BindingAction::B), "East");
    assert_eq!(restored.get_p2(BindingAction::L), "LeftTrigger");
    assert_eq!(restored.get_p2(BindingAction::R), "RightTrigger");
    assert_eq!(restored.get_p2(BindingAction::Up), "DPadUp");
    assert_eq!(restored.get_ws(WonderSwanButton::A), "South");
    assert_eq!(restored.get_ws(WonderSwanButton::B), "East");
    assert_eq!(restored.get_ws(WonderSwanButton::Start), "Start");
    assert_eq!(restored.get_ws(WonderSwanButton::X1), "");
}

#[test]
fn gamepad_bindings_migrate_old_empty_wonderswan_defaults_once() {
    let mut bindings = GamepadBindings {
        ws_a: String::new(),
        ws_b: String::new(),
        ws_start: String::new(),
        wonderswan_defaults_initialized: false,
        ..GamepadBindings::default()
    };

    bindings.migrate_wonderswan_defaults();

    assert_eq!(bindings.get_ws(WonderSwanButton::A), "South");
    assert_eq!(bindings.get_ws(WonderSwanButton::B), "East");
    assert_eq!(bindings.get_ws(WonderSwanButton::Start), "Start");
    assert!(bindings.wonderswan_defaults_initialized);

    bindings.clear_wonderswan_direct_bindings();
    bindings.migrate_wonderswan_defaults();

    assert_eq!(bindings.get_ws(WonderSwanButton::A), "");
    assert_eq!(bindings.get_ws(WonderSwanButton::B), "");
    assert_eq!(bindings.get_ws(WonderSwanButton::Start), "");
}

#[test]
fn tilt_key_bindings_serde_roundtrip() {
    let bindings = TiltKeyBindings {
        up: KeyCode::KeyI,
        ..TiltKeyBindings::default()
    };
    let json = serde_json::to_string(&bindings).unwrap();
    let restored: TiltKeyBindings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.up, KeyCode::KeyI);
    assert_eq!(restored.down, KeyCode::KeyS);
}

#[test]
fn recent_roms_add_and_dedup() {
    let mut s = Settings::default();
    s.add_recent_rom(Path::new("game1.gb"));
    s.add_recent_rom(Path::new("game2.gb"));
    s.add_recent_rom(Path::new("game1.gb"));
    assert_eq!(s.recent_roms.len(), 2);
    assert_eq!(s.recent_roms[0].name, "game1.gb");
    assert_eq!(s.recent_roms[1].name, "game2.gb");
}

#[test]
fn recent_roms_truncates_at_max() {
    let mut s = Settings::default();
    for i in 0..15 {
        s.add_recent_rom(Path::new(&format!("game{i}.gb")));
    }
    assert_eq!(s.recent_roms.len(), MAX_RECENT_ROMS);
}

#[test]
fn default_rewind_speed_is_3() {
    assert_eq!(Settings::default().rewind.speed, 3);
}

#[test]
fn pre_mute_volume_is_skipped_in_serde() {
    let mut s = Settings::default();
    s.audio.pre_mute_volume = Some(0.75);
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.audio.pre_mute_volume, None);
}

#[test]
fn audio_output_sample_rate_serde_roundtrip() {
    let mut s = Settings::default();
    s.audio.output_sample_rate = 44_100;
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.audio.output_sample_rate, 44_100);
}

#[test]
fn audio_output_sample_rate_defaults_when_missing() {
    let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.audio.output_sample_rate, 48_000);
}

#[test]
fn audio_low_pass_settings_serde_roundtrip() {
    let mut s = Settings::default();
    s.audio.low_pass_enabled = true;
    s.audio.low_pass_cutoff_hz = 2_400;
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert!(restored.audio.low_pass_enabled);
    assert_eq!(restored.audio.low_pass_cutoff_hz, 2_400);
}

#[test]
fn audio_low_pass_settings_defaults_when_missing() {
    let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert!(!s.audio.low_pass_enabled);
    assert_eq!(s.audio.low_pass_cutoff_hz, 4_800);
}

#[test]
fn shader_params_roundtrip() {
    let params = ShaderParams {
        scanline_intensity: 0.5,
        crt_curvature: 0.8,
        grid_intensity: 0.1,
        upscale_edge_strength: 0.75,
        palette_mix: 0.9,
        palette_warmth: 0.2,
    };
    let json = serde_json::to_string(&params).unwrap();
    let restored: ShaderParams = serde_json::from_str(&json).unwrap();
    assert_eq!(params, restored);
}

#[test]
fn shader_params_to_gpu_bytes() {
    let params = ShaderParams::default();
    let bytes = params.to_gpu_bytes();
    let scanline = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let curvature = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let edge = f32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let mix = f32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    assert!((scanline - params.scanline_intensity).abs() < f32::EPSILON);
    assert!((curvature - params.crt_curvature).abs() < f32::EPSILON);
    assert!((edge - params.upscale_edge_strength).abs() < f32::EPSILON);
    assert!((mix - params.palette_mix).abs() < f32::EPSILON);
}

#[test]
fn build_gpu_params_includes_color_correction() {
    let params = ShaderParams::default();
    let buf = build_gpu_params(
        &params,
        effective_gb_color_correction(ColorCorrection::GbcLcd, default_color_correction_matrix()),
        160.0,
        144.0,
    );
    let mode = u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]);
    assert_eq!(mode, 1);
    let r00 = f32::from_le_bytes([buf[48], buf[49], buf[50], buf[51]]);
    assert!((r00 - 26.0 / 32.0).abs() < f32::EPSILON);
}

#[test]
fn build_gpu_params_none_mode_is_identity() {
    let params = ShaderParams::default();
    let buf = build_gpu_params(&params, EffectiveColorCorrection::None, 160.0, 144.0);
    let mode = u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]);
    assert_eq!(mode, 0);
    let r00 = f32::from_le_bytes([buf[48], buf[49], buf[50], buf[51]]);
    assert!((r00 - 1.0).abs() < f32::EPSILON);
}

#[test]
fn rewind_capture_interval_is_4() {
    let s = Settings::default();
    assert_eq!(s.rewind.capture_interval(), 4);
}

#[test]
fn color_correction_serde_roundtrip() {
    let mut s = Settings::default();
    s.video.gb_color_correction = ColorCorrection::GbcLcd;
    s.video.gba_color_correction = GbaColorCorrection::LcdResponse;
    s.video.ws_color_correction = WonderSwanColorCorrection::MonoLcd;
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.video.gb_color_correction, ColorCorrection::GbcLcd);
    assert_eq!(
        restored.video.gba_color_correction,
        GbaColorCorrection::LcdResponse
    );
    assert_eq!(
        restored.video.ws_color_correction,
        WonderSwanColorCorrection::MonoLcd
    );
}

#[test]
fn color_correction_defaults_to_none_when_missing() {
    let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.video.gb_color_correction, ColorCorrection::None);
    assert_eq!(
        s.video.gb_color_correction_matrix,
        default_color_correction_matrix()
    );
    assert_eq!(s.video.gba_color_correction, GbaColorCorrection::None);
    assert_eq!(
        s.video.gba_color_correction_matrix,
        default_color_correction_matrix()
    );
    assert_eq!(s.video.ws_color_correction, WonderSwanColorCorrection::None);
    assert_eq!(
        s.video.ws_color_correction_matrix,
        default_color_correction_matrix()
    );
}

#[test]
fn custom_color_correction_matrix_roundtrip() {
    let mut s = Settings::default();
    s.video.gb_color_correction = ColorCorrection::Custom;
    s.video.gb_color_correction_matrix = [1.0, 0.2, 0.0, 0.1, 0.9, 0.0, 0.0, 0.3, 0.8];
    s.video.gba_color_correction = GbaColorCorrection::Custom;
    s.video.gba_color_correction_matrix = [0.8, 0.1, 0.1, 0.2, 0.7, 0.1, 0.0, 0.2, 0.8];
    s.video.ws_color_correction = WonderSwanColorCorrection::Custom;
    s.video.ws_color_correction_matrix = [0.7, 0.2, 0.1, 0.1, 0.8, 0.1, 0.2, 0.1, 0.7];
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.video.gb_color_correction, ColorCorrection::Custom);
    assert_eq!(
        restored.video.gba_color_correction,
        GbaColorCorrection::Custom
    );
    assert_eq!(
        restored.video.gb_color_correction_matrix,
        s.video.gb_color_correction_matrix
    );
    assert_eq!(
        restored.video.gba_color_correction_matrix,
        s.video.gba_color_correction_matrix
    );
    assert_eq!(
        restored.video.ws_color_correction,
        WonderSwanColorCorrection::Custom
    );
    assert_eq!(
        restored.video.ws_color_correction_matrix,
        s.video.ws_color_correction_matrix
    );
}

#[test]
fn legacy_color_correction_fields_load_as_game_boy_settings() {
    let json = r#"{
        "hardware_mode_preference":"Auto",
        "fast_forward_multiplier":4,
        "color_correction":"gbc_lcd",
        "color_correction_matrix":[1.0,0.2,0.0,0.1,0.9,0.0,0.0,0.3,0.8],
        "dmg_palette_preset":"mint"
    }"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.video.gb_color_correction, ColorCorrection::GbcLcd);
    assert_eq!(s.video.gb_dmg_palette_preset, DmgPalettePreset::Mint);
    assert_eq!(s.video.gba_color_correction, GbaColorCorrection::None);
}

#[test]
fn gba_color_correction_uses_dedicated_gpu_modes() {
    let params = ShaderParams::default();
    let agb = build_gpu_params(
        &params,
        effective_gba_color_correction(
            GbaColorCorrection::AgbLcd,
            default_color_correction_matrix(),
        ),
        240.0,
        160.0,
    );
    let lcd_response = build_gpu_params(
        &params,
        effective_gba_color_correction(
            GbaColorCorrection::LcdResponse,
            default_color_correction_matrix(),
        ),
        240.0,
        160.0,
    );

    assert_eq!(u32::from_le_bytes([agb[32], agb[33], agb[34], agb[35]]), 2);
    assert_eq!(
        u32::from_le_bytes([
            lcd_response[32],
            lcd_response[33],
            lcd_response[34],
            lcd_response[35]
        ]),
        3
    );
}

#[test]
fn wonderswan_color_correction_uses_lcd_modes() {
    let params = ShaderParams::default();
    let color_lcd = build_gpu_params(
        &params,
        effective_wonderswan_color_correction(
            WonderSwanColorCorrection::ColorLcd,
            default_color_correction_matrix(),
        ),
        224.0,
        144.0,
    );
    let mono_lcd = build_gpu_params(
        &params,
        effective_wonderswan_color_correction(
            WonderSwanColorCorrection::MonoLcd,
            default_color_correction_matrix(),
        ),
        224.0,
        144.0,
    );

    assert_eq!(
        u32::from_le_bytes([color_lcd[32], color_lcd[33], color_lcd[34], color_lcd[35]]),
        1
    );
    let r00 = f32::from_le_bytes([color_lcd[48], color_lcd[49], color_lcd[50], color_lcd[51]]);
    assert!((r00 - 26.0 / 32.0).abs() < f32::EPSILON);
    assert_eq!(
        u32::from_le_bytes([mono_lcd[32], mono_lcd[33], mono_lcd[34], mono_lcd[35]]),
        4
    );
}

#[test]
fn dmg_palette_preset_serde_roundtrip() {
    let mut s = Settings::default();
    s.video.gb_dmg_palette_preset = DmgPalettePreset::Mint;
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.video.gb_dmg_palette_preset, DmgPalettePreset::Mint);
}

#[test]
fn dmg_palette_preset_defaults_when_missing() {
    let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.video.gb_dmg_palette_preset, DmgPalettePreset::DmgGreen);
}

#[test]
fn nes_palette_mode_serde_roundtrip() {
    let mut s = Settings::default();
    s.video.nes_palette_mode = NesPaletteMode::Pal;
    s.video.nes_custom_palette_path = "C:/palettes/custom.pal".to_string();
    s.video.nes_custom_palette_name = "custom.pal".to_string();
    s.video.nes_custom_palette_bytes = vec![0, 1, 2, 3, 4, 5];
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.video.nes_palette_mode, NesPaletteMode::Pal);
    assert_eq!(
        restored.video.nes_custom_palette_path,
        "C:/palettes/custom.pal"
    );
    assert_eq!(restored.video.nes_custom_palette_name, "custom.pal");
    assert_eq!(
        restored.video.nes_custom_palette_bytes,
        vec![0, 1, 2, 3, 4, 5]
    );
}

#[test]
fn nes_palette_mode_defaults_to_raw_when_missing() {
    let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.video.nes_palette_mode, NesPaletteMode::Raw);
    assert!(s.video.nes_custom_palette_path.is_empty());
    assert!(s.video.nes_custom_palette_name.is_empty());
    assert!(s.video.nes_custom_palette_bytes.is_empty());
}

#[test]
fn vsync_mode_serde_roundtrip() {
    let mut s = Settings::default();
    s.video.vsync_mode = VsyncMode::Off;
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.video.vsync_mode, VsyncMode::Off);

    s.video.vsync_mode = VsyncMode::Adaptive;
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.video.vsync_mode, VsyncMode::Adaptive);
}

#[test]
fn vsync_mode_defaults_to_on_when_missing() {
    let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.video.vsync_mode, VsyncMode::On);
}

#[test]
fn camera_defaults_match_tuned_profile() {
    let s = Settings::default();
    assert_eq!(s.camera.device_index, 0);
    assert!(!s.camera.auto_levels);
    assert!((s.camera.brightness - 0.15).abs() < f32::EPSILON);
    assert!((s.camera.contrast - 1.65).abs() < f32::EPSILON);
    assert!((s.camera.gamma - 1.05).abs() < f32::EPSILON);
}
