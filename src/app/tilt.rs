use crate::settings::TiltInputMode;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AutoTiltSource {
    Keyboard,
    Mouse,
}

#[derive(Clone, Copy)]
pub(super) struct TiltConfig {
    pub(super) sensitivity: f32,
    pub(super) invert_x: bool,
    pub(super) invert_y: bool,
    pub(super) deadzone: f32,
    pub(super) stick_bypass_lerp: bool,
    pub(super) lerp: f32,
}

pub(super) fn mouse_tilt_vector(
    cursor_pos: Option<(f32, f32)>,
    window_size: (f32, f32),
) -> (f32, f32) {
    let Some((cursor_x, cursor_y)) = cursor_pos else {
        return (0.0, 0.0);
    };
    let (width, height) = window_size;
    if width <= 1.0 || height <= 1.0 {
        return (0.0, 0.0);
    }

    let center_x = width * 0.5;
    let center_y = height * 0.5;
    let half_min = (width.min(height) * 0.5).max(1.0);
    let x = ((cursor_x - center_x) / half_min).clamp(-1.0, 1.0);
    let y = ((center_y - cursor_y) / half_min).clamp(-1.0, 1.0);
    (x, y)
}

pub(super) struct TiltInputSources {
    pub(super) keyboard: (f32, f32),
    pub(super) mouse: (f32, f32),
    pub(super) left_stick: (f32, f32),
}

pub(super) fn compute_target_tilt(
    is_mbc7: bool,
    mode: TiltInputMode,
    auto_source: &mut AutoTiltSource,
    inputs: &TiltInputSources,
    stick_controls_tilt: bool,
    cfg: &TiltConfig,
) -> (f32, f32) {
    if !is_mbc7 {
        return (0.0, 0.0);
    }

    if mode == TiltInputMode::Auto {
        if inputs.keyboard.0.abs() > 0.01 || inputs.keyboard.1.abs() > 0.01 {
            *auto_source = AutoTiltSource::Keyboard;
        } else if inputs.mouse.0.abs() > 0.15 || inputs.mouse.1.abs() > 0.15 {
            *auto_source = AutoTiltSource::Mouse;
        }
    }

    let use_keyboard = match mode {
        TiltInputMode::Keyboard => true,
        TiltInputMode::Mouse => false,
        TiltInputMode::Auto => *auto_source == AutoTiltSource::Keyboard,
    };

    let use_mouse = match mode {
        TiltInputMode::Keyboard => false,
        TiltInputMode::Mouse => true,
        TiltInputMode::Auto => *auto_source == AutoTiltSource::Mouse,
    };

    let mut x = 0.0;
    let mut y = 0.0;

    if use_keyboard {
        x += inputs.keyboard.0;
        y += inputs.keyboard.1;
    }

    if use_mouse {
        x += inputs.mouse.0;
        y += inputs.mouse.1;
    }

    if stick_controls_tilt {
        x += inputs.left_stick.0;
        y += inputs.left_stick.1;
    }

    if cfg.invert_x {
        x = -x;
    }
    if cfg.invert_y {
        y = -y;
    }

    x = (x * cfg.sensitivity).clamp(-1.0, 1.0);
    y = (y * cfg.sensitivity).clamp(-1.0, 1.0);
    (x, y)
}

pub(super) fn update_smoothed_tilt(
    current: &mut (f32, f32),
    target: (f32, f32),
    is_mbc7: bool,
    left_stick: (f32, f32),
    stick_controls_tilt: bool,
    cfg: &TiltConfig,
) -> (f32, f32) {
    if !is_mbc7 {
        *current = (0.0, 0.0);
        return *current;
    }

    let stick_active = stick_controls_tilt
        && (left_stick.0.abs() >= cfg.deadzone || left_stick.1.abs() >= cfg.deadzone);

    if stick_active && cfg.stick_bypass_lerp {
        *current = target;
        return *current;
    }

    let alpha = cfg.lerp.clamp(0.0, 1.0);
    current.0 += (target.0 - current.0) * alpha;
    current.1 += (target.1 - current.1) * alpha;
    *current
}
