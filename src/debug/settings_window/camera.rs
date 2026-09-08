use crate::{
    camera::{host_camera_supported, query_host_cameras},
    debug::DebugWindowState,
    settings::Settings,
};

use super::{
    layout::{checkbox, helper, row},
    search::{self, SettingId as Id},
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

    helper(
        ui,
        egui::RichText::new("Choose the host camera used by supported games.")
            .small()
            .weak(),
    );
    ui.add_space(12.0);
    if row(ui, Id::CameraRefreshCameraDevices, None, |ui| {
        ui.button("Refresh devices")
    })
    .clicked()
    {
        state.camera_devices_needs_refresh = true;
    }
    if !host_camera_supported() {
        helper(
            ui,
            egui::RichText::new("Host camera unavailable in this build").weak(),
        );
    }

    let selected_label = state
        .camera_devices
        .iter()
        .find(|device| device.index == settings.camera.device_index)
        .map(|device| format!("{} ({})", device.name, device.index))
        .unwrap_or_else(|| format!("Camera {}", settings.camera.device_index));
    row(ui, Id::CameraCameraDevice, None, |ui| {
        egui::ComboBox::from_id_salt("camera_device")
            .selected_text(selected_label)
            .width(240.0)
            .show_ui(ui, |ui| {
                for device in &state.camera_devices {
                    ui.selectable_value(
                        &mut settings.camera.device_index,
                        device.index,
                        format!("{} ({})", device.name, device.index),
                    );
                }
            })
            .response
    });

    egui::CollapsingHeader::new("Advanced device selection")
        .open(search::requested(ui, Id::CameraCameraDeviceIndex).then_some(true))
        .show(ui, |ui| {
            row(ui, Id::CameraCameraDeviceIndex, None, |ui| {
                ui.add(
                    egui::DragValue::new(&mut settings.camera.device_index)
                        .range(0..=64)
                        .speed(1),
                )
            });
        });

    if let Some(err) = &state.camera_device_error {
        helper(ui, egui::RichText::new(err).small().weak());
    }

    ui.add_space(22.0);
    ui.heading("Image tuning");
    checkbox(
        ui,
        Id::CameraAutomaticLevels,
        None,
        &mut settings.camera.auto_levels,
    );
    row(ui, Id::CameraCameraBrightness, None, |ui| {
        ui.add_sized(
            [240.0, 30.0],
            egui::Slider::new(&mut settings.camera.brightness, -1.0..=1.0).step_by(0.01),
        )
    });
    row(ui, Id::CameraCameraContrast, None, |ui| {
        ui.add_sized(
            [240.0, 30.0],
            egui::Slider::new(&mut settings.camera.contrast, 0.25..=3.0).step_by(0.01),
        )
    });
    row(ui, Id::CameraCameraGamma, None, |ui| {
        ui.add_sized(
            [240.0, 30.0],
            egui::Slider::new(&mut settings.camera.gamma, 0.4..=2.5).step_by(0.01),
        )
    });
    ui.indent("camera_tuning_reset", |ui| {
        if row(ui, Id::CameraResetCameraTuning, None, |ui| {
            ui.small_button("Reset camera tuning")
        })
        .clicked()
        {
            settings.camera.auto_levels = false;
            settings.camera.brightness = 0.15;
            settings.camera.contrast = 1.65;
            settings.camera.gamma = 1.05;
        }
    });
}
