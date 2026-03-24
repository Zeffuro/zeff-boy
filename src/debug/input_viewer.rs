use crate::debug::DebugInfo;

pub(super) fn draw_input_viewer_content(ui: &mut egui::Ui, info: &DebugInfo) {
    ui.heading("Input State");

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

    ui.separator();
    ui.heading("Tilt Sources");
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
    ui.heading("Tilt Output");
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
}
