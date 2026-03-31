use super::*;

#[test]
fn settings_default_roundtrip() {
    let defaults = Settings::default();
    let json = serde_json::to_string_pretty(&defaults).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(defaults, restored);
}

#[test]
fn settings_with_modified_values_roundtrip() {
    let mut s = Settings::default();
    s.emulation.fast_forward_multiplier = 8;
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
}

#[test]
fn key_bindings_serde_roundtrip() {
    let mut bindings = KeyBindings::default();
    bindings.a = KeyCode::KeyQ;
    bindings.b = KeyCode::KeyE;

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
}

#[test]
fn shortcut_bindings_get_returns_default_for_unknown_string() {
    let mut bindings = ShortcutBindings::default();
    bindings.fullscreen = "NONSENSE".to_string();

    assert_eq!(bindings.get(ShortcutAction::Fullscreen), KeyCode::F11);
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
    let json = serde_json::to_string(&gb).unwrap();
    let restored: GamepadBindings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.get(BindingAction::A), "West");
    assert_eq!(restored.get(BindingAction::B), "East");
}

#[test]
fn tilt_key_bindings_serde_roundtrip() {
    let mut bindings = TiltKeyBindings::default();
    bindings.up = KeyCode::KeyI;
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
        ColorCorrection::GbcLcd,
        default_color_correction_matrix(),
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
    let buf = build_gpu_params(
        &params,
        ColorCorrection::None,
        default_color_correction_matrix(),
        160.0,
        144.0,
    );
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
    s.video.color_correction = ColorCorrection::GbcLcd;
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.video.color_correction, ColorCorrection::GbcLcd);
}

#[test]
fn color_correction_defaults_to_none_when_missing() {
    let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.video.color_correction, ColorCorrection::None);
    assert_eq!(
        s.video.color_correction_matrix,
        default_color_correction_matrix()
    );
}

#[test]
fn custom_color_correction_matrix_roundtrip() {
    let mut s = Settings::default();
    s.video.color_correction = ColorCorrection::Custom;
    s.video.color_correction_matrix = [1.0, 0.2, 0.0, 0.1, 0.9, 0.0, 0.0, 0.3, 0.8];
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.video.color_correction, ColorCorrection::Custom);
    assert_eq!(
        restored.video.color_correction_matrix,
        s.video.color_correction_matrix
    );
}

#[test]
fn dmg_palette_preset_serde_roundtrip() {
    let mut s = Settings::default();
    s.video.dmg_palette_preset = DmgPalettePreset::Mint;
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.video.dmg_palette_preset, DmgPalettePreset::Mint);
}

#[test]
fn dmg_palette_preset_defaults_when_missing() {
    let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.video.dmg_palette_preset, DmgPalettePreset::DmgGreen);
}

#[test]
fn nes_palette_mode_serde_roundtrip() {
    let mut s = Settings::default();
    s.video.nes_palette_mode = NesPaletteMode::Pal;
    let json = serde_json::to_string(&s).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.video.nes_palette_mode, NesPaletteMode::Pal);
}

#[test]
fn nes_palette_mode_defaults_to_raw_when_missing() {
    let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.video.nes_palette_mode, NesPaletteMode::Raw);
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
