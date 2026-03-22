use crate::settings::TiltInputMode;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AutoTiltSource {
    Keyboard,
    Mouse,
}

#[derive(Clone, Copy)]
pub(super) struct TiltFrameData {
    pub(super) is_mbc7: bool,
    pub(super) stick_controls_tilt: bool,
    pub(super) keyboard: (f32, f32),
    pub(super) mouse: (f32, f32),
    pub(super) left_stick: (f32, f32),
    pub(super) target: (f32, f32),
    pub(super) smoothed: (f32, f32),
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

#[allow(clippy::too_many_arguments)]
pub(super) fn compute_target_tilt(
    is_mbc7: bool,
    mode: TiltInputMode,
    auto_source: &mut AutoTiltSource,
    keyboard: (f32, f32),
    mouse: (f32, f32),
    left_stick: (f32, f32),
    stick_controls_tilt: bool,
    tilt_sensitivity: f32,
    tilt_invert_x: bool,
    tilt_invert_y: bool,
) -> (f32, f32) {
    if !is_mbc7 {
        return (0.0, 0.0);
    }

    if mode == TiltInputMode::Auto {
        if keyboard.0.abs() > 0.01 || keyboard.1.abs() > 0.01 {
            *auto_source = AutoTiltSource::Keyboard;
        } else if mouse.0.abs() > 0.15 || mouse.1.abs() > 0.15 {
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
        x += keyboard.0;
        y += keyboard.1;
    }

    if use_mouse {
        x += mouse.0;
        y += mouse.1;
    }

    if stick_controls_tilt {
        x += left_stick.0;
        y += left_stick.1;
    }

    if tilt_invert_x {
        x = -x;
    }
    if tilt_invert_y {
        y = -y;
    }

    x = (x * tilt_sensitivity).clamp(-1.0, 1.0);
    y = (y * tilt_sensitivity).clamp(-1.0, 1.0);
    (x, y)
}

pub(super) fn update_smoothed_tilt(
    current: &mut (f32, f32),
    target: (f32, f32),
    is_mbc7: bool,
    left_stick: (f32, f32),
    stick_controls_tilt: bool,
    deadzone: f32,
    stick_bypass_lerp: bool,
    lerp: f32,
) -> (f32, f32) {
    if !is_mbc7 {
        *current = (0.0, 0.0);
        return *current;
    }

    let stick_active =
        stick_controls_tilt && (left_stick.0.abs() >= deadzone || left_stick.1.abs() >= deadzone);

    if stick_active && stick_bypass_lerp {
        *current = target;
        return *current;
    }

    let alpha = lerp.clamp(0.0, 1.0);
    current.0 += (target.0 - current.0) * alpha;
    current.1 += (target.1 - current.1) * alpha;
    *current
}
