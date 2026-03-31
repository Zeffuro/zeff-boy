use crate::debug::{DebugSection, InputDebugInfo};

pub(super) fn gb_input_snapshot(info: &zeff_gb_core::debug::DebugInfo) -> InputDebugInfo {
    let sections = vec![
        DebugSection {
            heading: "Input State".into(),
            lines: vec![
                format!(
                    "MBC7 active: {}",
                    if info.tilt_is_mbc7 { "yes" } else { "no" }
                ),
                format!(
                    "Left stick routes to: {}",
                    if info.tilt_stick_controls_tilt {
                        "tilt"
                    } else {
                        "d-pad"
                    }
                ),
            ],
        },
        DebugSection {
            heading: "Tilt Sources".into(),
            lines: vec![
                format!(
                    "Keyboard  x:{:>6.2} y:{:>6.2}",
                    info.tilt_keyboard.0, info.tilt_keyboard.1
                ),
                format!(
                    "Mouse     x:{:>6.2} y:{:>6.2}",
                    info.tilt_mouse.0, info.tilt_mouse.1
                ),
                format!(
                    "LeftStick x:{:>6.2} y:{:>6.2}",
                    info.tilt_left_stick.0, info.tilt_left_stick.1
                ),
            ],
        },
        DebugSection {
            heading: "Tilt Output".into(),
            lines: vec![
                format!(
                    "Target    x:{:>6.2} y:{:>6.2}",
                    info.tilt_target.0, info.tilt_target.1
                ),
                format!(
                    "Smoothed  x:{:>6.2} y:{:>6.2}",
                    info.tilt_smoothed.0, info.tilt_smoothed.1
                ),
            ],
        },
    ];
    let smoothed_x = ((info.tilt_smoothed.0 + 1.0) * 0.5).clamp(0.0, 1.0);
    let smoothed_y = ((info.tilt_smoothed.1 + 1.0) * 0.5).clamp(0.0, 1.0);
    InputDebugInfo {
        sections,
        progress_bars: vec![
            ("Smoothed X (-1..1)".into(), smoothed_x),
            ("Smoothed Y (-1..1)".into(), smoothed_y),
        ],
    }
}
