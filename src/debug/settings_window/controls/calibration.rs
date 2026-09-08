use super::super::search::{self, SettingId};
use crate::input::{GamepadDeviceSnapshot, RuntimeGamepadId};
use crate::settings::{
    AxisCalibration, GamepadCalibration, GamepadFingerprint, MIN_AXIS_SPAN, Settings,
    StickCalibration,
};

const CENTER_SAMPLE_COUNT: u32 = 30;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CalibrationPhase {
    Center,
    ReadyToMeasureRange,
    MeasuringRange,
}

#[derive(Clone)]
pub(crate) struct CalibrationCapture {
    device: RuntimeGamepadId,
    fingerprint: GamepadFingerprint,
    phase: CalibrationPhase,
    draft: GamepadCalibration,
    last_generation: u64,
    center_sum: [f32; 4],
    center_samples: u32,
    range_samples: u32,
    left_range_active: bool,
    right_range_active: bool,
}

impl CalibrationCapture {
    fn start(device: &GamepadDeviceSnapshot, generation: u64) -> Self {
        Self {
            device: device.id,
            fingerprint: device.fingerprint.clone(),
            phase: CalibrationPhase::Center,
            draft: GamepadCalibration::default(),
            last_generation: generation,
            center_sum: [0.0; 4],
            center_samples: 0,
            range_samples: 0,
            left_range_active: true,
            right_range_active: true,
        }
    }

    fn sample(&mut self, device: &GamepadDeviceSnapshot, generation: u64) {
        if device.id != self.device || generation == self.last_generation {
            return;
        }
        self.last_generation = generation;
        let sample = [
            device.left_stick.0,
            device.left_stick.1,
            device.right_stick.0,
            device.right_stick.1,
        ];
        match self.phase {
            CalibrationPhase::Center => {
                for (sum, value) in self.center_sum.iter_mut().zip(sample) {
                    *sum += value;
                }
                self.center_samples = self.center_samples.saturating_add(1);
                if self.center_samples >= CENTER_SAMPLE_COUNT {
                    let center = self.center_sum.map(|sum| sum / self.center_samples as f32);
                    self.draft.left.x.center = center[0];
                    self.draft.left.y.center = center[1];
                    self.draft.right.x.center = center[2];
                    self.draft.right.y.center = center[3];
                    self.phase = CalibrationPhase::ReadyToMeasureRange;
                }
            }
            CalibrationPhase::ReadyToMeasureRange => {}
            CalibrationPhase::MeasuringRange => {
                if self.left_range_active {
                    observe_stick_range(&mut self.draft.left, (sample[0], sample[1]));
                }
                if self.right_range_active {
                    observe_stick_range(&mut self.draft.right, (sample[2], sample[3]));
                }
                self.range_samples = self.range_samples.saturating_add(1);
            }
        }
    }

    fn begin_range(&mut self) {
        reset_stick_ranges(&mut self.draft.left);
        reset_stick_ranges(&mut self.draft.right);
        self.range_samples = 0;
        self.left_range_active = true;
        self.right_range_active = true;
        self.phase = CalibrationPhase::MeasuringRange;
    }

    fn is_valid(&self) -> bool {
        self.draft.is_valid()
    }
}

pub(crate) fn draw(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    settings_ui: &mut super::SettingsUiState,
    selected: Option<&GamepadDeviceSnapshot>,
) {
    ui.add_space(12.0);
    ui.separator();
    ui.heading("Stick calibration");
    ui.label(
        egui::RichText::new(
            "Calibration is stored for this controller model hint. Controllers that report the same hint share it.",
        )
        .weak(),
    );

    let phase = settings_ui
        .calibration_capture
        .as_ref()
        .map(|capture| capture.phase);
    for (id, available) in [
        (
            SettingId::InputDevicesMeasureRangeCalibration,
            phase == Some(CalibrationPhase::ReadyToMeasureRange),
        ),
        (
            SettingId::InputDevicesApplyCalibration,
            phase == Some(CalibrationPhase::MeasuringRange),
        ),
        (SettingId::InputDevicesCancelCalibration, phase.is_some()),
        (SettingId::InputDevicesResetCalibration, selected.is_some()),
    ] {
        search::conditional(
            ui,
            id,
            SettingId::InputDevicesStartCalibration,
            available,
            "Choose a connected controller and complete the calibration steps to use this action.",
        );
    }
    let Some(device) = selected else {
        let response =
            ui.label(egui::RichText::new("Choose a connected controller to calibrate it.").weak());
        search::target(ui, SettingId::InputDevicesStartCalibration, &response);
        return;
    };

    let fingerprint = device.fingerprint.clone();
    if settings_ui
        .calibration_capture
        .as_ref()
        .is_some_and(|capture| capture.device != device.id)
    {
        settings_ui.calibration_capture = None;
        settings_ui.calibration_notice =
            Some("Calibration capture was cancelled after selecting another controller.".into());
    }
    if let Some(capture) = &mut settings_ui.calibration_capture {
        capture.sample(device, settings_ui.gamepad_snapshot.sample_generation);
    }

    if let Some(notice) = &settings_ui.calibration_notice {
        ui.label(egui::RichText::new(notice).color(egui::Color32::YELLOW));
    }

    let existing = settings.input_devices.calibration_for(&fingerprint);
    ui.horizontal_wrapped(|ui| {
        let start = ui.button("Start center sampling");
        search::target(ui, SettingId::InputDevicesStartCalibration, &start);
        if start.clicked() {
            settings_ui.calibration_capture = Some(CalibrationCapture::start(
                device,
                settings_ui.gamepad_snapshot.sample_generation,
            ));
            settings_ui.calibration_notice =
                Some("Leave both sticks untouched while 30 fresh samples are collected.".into());
        }
        let reset = ui.add_enabled(existing.is_some(), egui::Button::new("Reset calibration"));
        search::target(ui, SettingId::InputDevicesResetCalibration, &reset);
        if reset.clicked() && settings.input_devices.remove_calibration(&fingerprint) {
            settings_ui.calibration_capture = None;
            settings_ui.calibration_notice = Some("Removed the saved model calibration.".into());
        }
    });

    let mut apply = false;
    let mut cancel = false;
    if let Some(capture) = &mut settings_ui.calibration_capture {
        match capture.phase {
            CalibrationPhase::Center => {
                ui.label(format!(
                    "Center sampling: {}/{} fresh samples",
                    capture.center_samples, CENTER_SAMPLE_COUNT
                ));
            }
            CalibrationPhase::ReadyToMeasureRange => {
                ui.label("Center captured. Move both sticks fully in every direction, then begin range measurement.");
                let range = ui.button("Measure range");
                search::target(ui, SettingId::InputDevicesMeasureRangeCalibration, &range);
                if range.clicked() {
                    capture.begin_range();
                    settings_ui.calibration_notice = Some(
                        "Measuring raw extrema. Move each stick fully through its range, then refine values if needed."
                            .into(),
                    );
                }
            }
            CalibrationPhase::MeasuringRange => {
                ui.label(format!("Range samples: {}", capture.range_samples));
                draw_range_stick_editor(
                    ui,
                    "Left stick",
                    &mut capture.draft.left,
                    &mut capture.left_range_active,
                );
                draw_range_stick_editor(
                    ui,
                    "Right stick",
                    &mut capture.draft.right,
                    &mut capture.right_range_active,
                );
                if !capture.is_valid() {
                    ui.label(
                        egui::RichText::new(format!(
                            "Each axis needs a finite min < center < max, with at least {:.2} range on each side.",
                            MIN_AXIS_SPAN
                        ))
                        .color(egui::Color32::YELLOW),
                    );
                }
                let response =
                    ui.add_enabled(capture.is_valid(), egui::Button::new("Apply calibration"));
                search::target(ui, SettingId::InputDevicesApplyCalibration, &response);
                apply = response.clicked();
            }
        }
        let response = ui.button("Cancel calibration");
        search::target(ui, SettingId::InputDevicesCancelCalibration, &response);
        cancel = response.clicked();
    } else if let Some(calibration) = existing {
        ui.label(
            egui::RichText::new(format!(
                "Saved calibration: left center ({:+.2}, {:+.2}), right center ({:+.2}, {:+.2}).",
                calibration.left.x.center,
                calibration.left.y.center,
                calibration.right.x.center,
                calibration.right.y.center,
            ))
            .weak(),
        );
    }

    if apply {
        if let Some(capture) = settings_ui.calibration_capture.take() {
            settings
                .input_devices
                .set_calibration(capture.fingerprint, capture.draft);
            settings_ui.calibration_notice =
                Some("Saved calibration for this controller model hint.".into());
        }
    } else if cancel {
        settings_ui.calibration_capture = None;
        settings_ui.calibration_notice = Some("Calibration changes were discarded.".into());
    }
}

fn reset_stick_ranges(stick: &mut StickCalibration) {
    stick.x.min = stick.x.center;
    stick.x.max = stick.x.center;
    stick.y.min = stick.y.center;
    stick.y.max = stick.y.center;
}

fn observe_stick_range(stick: &mut StickCalibration, raw: (f32, f32)) {
    observe_axis_range(&mut stick.x, raw.0);
    observe_axis_range(&mut stick.y, raw.1);
}

fn observe_axis_range(axis: &mut AxisCalibration, raw: f32) {
    let raw = if raw.is_finite() {
        raw.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    axis.min = axis.min.min(raw);
    axis.max = axis.max.max(raw);
}

fn draw_stick_editor(ui: &mut egui::Ui, name: &str, stick: &mut StickCalibration) {
    ui.add_space(6.0);
    ui.strong(name);
    ui.horizontal_wrapped(|ui| {
        draw_axis_editor(ui, "X", &mut stick.x);
        draw_axis_editor(ui, "Y", &mut stick.y);
    });
}

fn draw_range_stick_editor(
    ui: &mut egui::Ui,
    name: &str,
    stick: &mut StickCalibration,
    range_active: &mut bool,
) {
    ui.horizontal_wrapped(|ui| {
        if *range_active {
            if ui
                .button(format!("Keep default calibration for {name}"))
                .clicked()
            {
                *range_active = false;
                *stick = StickCalibration::default();
            }
        } else if ui.button(format!("Measure {name}")).clicked() {
            *range_active = true;
            reset_stick_ranges(stick);
        }
    });
    if *range_active {
        draw_stick_editor(ui, name, stick);
    } else {
        ui.label(egui::RichText::new(format!("{name} uses the default calibration.")).weak());
    }
}

fn draw_axis_editor(ui: &mut egui::Ui, name: &str, axis: &mut AxisCalibration) {
    ui.vertical(|ui| {
        ui.label(name);
        ui.add(egui::Slider::new(&mut axis.min, -1.0..=1.0).text("Min"));
        ui.add(egui::Slider::new(&mut axis.center, -1.0..=1.0).text("Center"));
        ui.add(egui::Slider::new(&mut axis.max, -1.0..=1.0).text("Max"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> GamepadDeviceSnapshot {
        GamepadDeviceSnapshot {
            id: RuntimeGamepadId(1),
            fingerprint: GamepadFingerprint {
                name: "Controller".into(),
                uuid: "model".into(),
            },
            buttons: Vec::new(),
            left_stick: (0.1, -0.1),
            right_stick: (0.2, -0.2),
            calibrated_left_stick: (0.0, 0.0),
            calibrated_right_stick: (0.0, 0.0),
            waiting_for_neutral: false,
        }
    }

    #[test]
    fn center_sampling_uses_each_snapshot_generation_once() {
        let device = device();
        let mut capture = CalibrationCapture::start(&device, 7);
        capture.sample(&device, 7);
        assert_eq!(capture.center_samples, 0);
        for generation in 8..=37 {
            capture.sample(&device, generation);
        }
        assert_eq!(capture.phase, CalibrationPhase::ReadyToMeasureRange);
        assert!((capture.draft.left.x.center - 0.1).abs() < 1e-6);
        assert!((capture.draft.right.y.center + 0.2).abs() < 1e-6);
    }

    #[test]
    fn range_measurement_starts_at_center_and_tracks_extrema() {
        let mut capture = CalibrationCapture::start(&device(), 0);
        capture.draft.left.x.center = 0.1;
        capture.draft.left.y.center = -0.1;
        capture.draft.right.x.center = 0.2;
        capture.draft.right.y.center = -0.2;
        capture.phase = CalibrationPhase::ReadyToMeasureRange;
        capture.begin_range();
        let mut min = device();
        min.left_stick = (-0.8, -0.7);
        min.right_stick = (-0.6, -0.5);
        capture.sample(&min, 1);
        let mut max = device();
        max.left_stick = (0.9, 0.6);
        max.right_stick = (0.7, 0.8);
        capture.sample(&max, 2);
        assert_eq!(capture.draft.left.x.min, -0.8);
        assert_eq!(capture.draft.left.x.max, 0.9);
        assert_eq!(capture.draft.right.y.min, -0.5);
        assert_eq!(capture.draft.right.y.max, 0.8);
        assert!(capture.is_valid());
    }

    #[test]
    fn either_stick_can_keep_default_calibration() {
        let mut capture = CalibrationCapture::start(&device(), 0);
        capture.draft.left.x.center = 0.1;
        capture.draft.left.y.center = -0.1;
        capture.draft.right.x.center = 0.2;
        capture.draft.right.y.center = -0.2;
        capture.phase = CalibrationPhase::ReadyToMeasureRange;
        capture.begin_range();
        capture.right_range_active = false;
        capture.draft.right = StickCalibration::default();
        let mut min = device();
        min.left_stick = (-0.8, -0.7);
        capture.sample(&min, 1);
        let mut max = device();
        max.left_stick = (0.9, 0.6);
        capture.sample(&max, 2);
        assert!(capture.is_valid());
        assert_eq!(capture.draft.right, StickCalibration::default());
    }
}
