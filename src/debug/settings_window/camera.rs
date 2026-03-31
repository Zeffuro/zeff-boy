use crate::{
    camera::{host_camera_supported, query_host_cameras},
    debug::DebugWindowState,
    settings::Settings,
};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    if state.camera_devices_needs_refresh {
        match query_host_cameras() {
            Ok(devices) => {
                state.camera_devices = devices;
                state.camera_device_error = None;
            }
            Err(err) => {
                state.camera_devices.clear();
                state.camera_device_error = Some(err.to_string());
            }
        }
        state.camera_devices_needs_refresh = false;
    }

    ui.heading("Camera");
    ui.label(
        egui::RichText::new(
            "Host-camera tuning for Pocket Camera input. Use these controls to avoid over-driving the in-game exposure slider.",
        )
        .small()
        .weak(),
    );

    ui.separator();
    ui.heading("Host Device");
    ui.horizontal(|ui| {
        if ui.button("Refresh devices").clicked() {
            state.camera_devices_needs_refresh = true;
        }
        if !host_camera_supported() {
            ui.label(egui::RichText::new("Host camera unavailable in this build").weak());
        }
    });

    let selected_label = state
        .camera_devices
        .iter()
        .find(|d| d.index == settings.camera.device_index)
        .map(|d| format!("{} ({})", d.name, d.index))
        .unwrap_or_else(|| format!("Camera {}", settings.camera.device_index));
    egui::ComboBox::from_label("Camera device")
        .selected_text(selected_label)
        .show_ui(ui, |ui| {
            for dev in &state.camera_devices {
                ui.selectable_value(
                    &mut settings.camera.device_index,
                    dev.index,
                    format!("{} ({})", dev.name, dev.index),
                );
            }
        });

    ui.add(
        egui::DragValue::new(&mut settings.camera.device_index)
            .range(0..=64)
            .speed(1),
    )
    .on_hover_text("Manual camera index override in case enumeration labels are wrong.");

    if let Some(err) = &state.camera_device_error {
        ui.label(egui::RichText::new(err).small().weak());
    }

    ui.checkbox(&mut settings.camera.auto_levels, "Auto-levels")
        .on_hover_text("Stretches luma histogram to use the full 0-255 range before quantization.");

    ui.add(
        egui::Slider::new(&mut settings.camera.brightness, -1.0..=1.0)
            .text("Brightness")
            .step_by(0.01),
    )
    .on_hover_text("Applies a linear brightness offset before gamma.");

    ui.add(
        egui::Slider::new(&mut settings.camera.contrast, 0.25..=3.0)
            .text("Contrast")
            .step_by(0.01),
    )
    .on_hover_text("Scales distance from mid-gray before gamma.");

    ui.add(
        egui::Slider::new(&mut settings.camera.gamma, 0.4..=2.5)
            .text("Gamma")
            .step_by(0.01),
    )
    .on_hover_text("Gamma correction on the grayscale frame.");

    if ui.button("Reset camera tuning").clicked() {
        settings.camera.auto_levels = false;
        settings.camera.brightness = 0.15;
        settings.camera.contrast = 1.65;
        settings.camera.gamma = 1.05;
    }

    ui.separator();
    ui.label(
        egui::RichText::new(
            "Host-camera support requires a camera-enabled build. Non-camera builds fall back to checkerboard test pattern.",
        )
        .small()
        .weak(),
    );
}
