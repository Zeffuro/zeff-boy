use anyhow::{Result, bail};

use super::{Settings, TiltSettings};

pub(super) fn validate(settings: &Settings) -> Result<()> {
    settings
        .input_overrides
        .validate()
        .map_err(anyhow::Error::msg)?;
    validate_usize("global.rewind.seconds", settings.rewind.seconds, 1, 120)?;
    validate_usize("global.rewind.speed", settings.rewind.speed, 1, 10)?;
    validate_usize(
        "global.emulation.fast_forward_multiplier",
        settings.emulation.fast_forward_multiplier,
        1,
        16,
    )?;
    validate_usize(
        "global.emulation.slow_motion_divisor",
        settings.emulation.slow_motion_divisor,
        2,
        16,
    )?;
    validate_usize(
        "global.emulation.uncapped_frames_per_tick",
        settings.emulation.uncapped_frames_per_tick,
        1,
        240,
    )?;

    let sample_rate = settings.audio.output_sample_rate;
    if !(8_000..=384_000).contains(&sample_rate) {
        bail!(
            "global.audio.output_sample_rate must be between 8000 and 384000 Hz (got {sample_rate})"
        );
    }
    validate_f32("global.audio.volume", settings.audio.volume, 0.0, 1.0)?;
    validate_f32("global.interface.ui_scale", settings.ui.ui_scale, 0.5, 3.0)?;
    validate_f32(
        "global.emulation.pce_mouse_sensitivity",
        settings.emulation.pce_mouse_sensitivity,
        0.25,
        4.0,
    )?;
    validate_tilt("global.input.tilt", &settings.tilt)?;
    if settings.input_devices.calibrations.len() > 64 {
        bail!("input_devices.calibrations supports at most 64 controller models");
    }
    for (index, calibration) in settings.input_devices.calibrations.iter().enumerate() {
        if !calibration.calibration.is_valid() {
            bail!("input_devices.calibrations[{index}] requires finite min/center/max ranges");
        }
    }

    for (index, profile) in settings.input_profiles.profiles.iter().enumerate() {
        let mut overrides = super::InputOverrides::default();
        overrides.global_keyboard_unbound = profile.bindings.keyboard_unbound.clone();
        overrides.global_binding_sets = profile.bindings.binding_sets.clone();
        overrides.global_autofire = profile.bindings.autofire.clone();
        overrides.validate().map_err(anyhow::Error::msg)?;
        validate_tilt(
            &format!("input_profiles.profiles[{index}].bindings.tilt"),
            &profile.bindings.tilt,
        )?;
    }

    Ok(())
}

fn validate_tilt(path: &str, tilt: &TiltSettings) -> Result<()> {
    // Stick/mouse input is normalized before these transforms. Sensitivity may
    // intentionally exceed the UI's 3x choice, but remains bounded to keep bad
    // documents from feeding extreme values into smoothing math.
    validate_f32(&format!("{path}.sensitivity"), tilt.sensitivity, 0.0, 16.0)?;
    validate_f32(&format!("{path}.lerp"), tilt.lerp, 0.0, 1.0)?;
    validate_f32(&format!("{path}.deadzone"), tilt.deadzone, 0.0, 1.0)
}

fn validate_usize(path: &str, value: usize, min: usize, max: usize) -> Result<()> {
    if !(min..=max).contains(&value) {
        bail!("{path} must be between {min} and {max} (got {value})");
    }
    Ok(())
}

fn validate_f32(path: &str, value: f32, min: f32, max: f32) -> Result<()> {
    if !value.is_finite() || value < min || value > max {
        bail!("{path} must be a finite value between {min} and {max} (got {value})");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_resource_extremes_and_names_the_invalid_field() {
        let mut settings = Settings::default();
        settings.rewind.seconds = usize::MAX;
        assert!(
            validate(&settings)
                .unwrap_err()
                .to_string()
                .contains("global.rewind.seconds")
        );

        let mut settings = Settings::default();
        settings.emulation.uncapped_frames_per_tick = 0;
        assert!(
            validate(&settings)
                .unwrap_err()
                .to_string()
                .contains("uncapped_frames_per_tick")
        );

        let mut settings = Settings::default();
        settings.audio.output_sample_rate = u32::MAX;
        assert!(
            validate(&settings)
                .unwrap_err()
                .to_string()
                .contains("output_sample_rate")
        );
    }

    #[test]
    fn rejects_non_finite_and_out_of_domain_calibration_in_profiles() {
        let mut settings = Settings::default();
        settings.save_input_profile("Invalid tilt").unwrap();
        settings.input_profiles.profiles[0]
            .bindings
            .tilt
            .sensitivity = f32::INFINITY;
        let error = validate(&settings).unwrap_err().to_string();
        assert!(error.contains("input_profiles.profiles[0].bindings.tilt.sensitivity"));

        settings.input_profiles.profiles[0]
            .bindings
            .tilt
            .sensitivity = 1.0;
        settings.input_profiles.profiles[0].bindings.tilt.deadzone = 1.01;
        assert!(
            validate(&settings)
                .unwrap_err()
                .to_string()
                .contains("deadzone")
        );
    }

    #[test]
    fn accepts_valid_nondefault_values_beyond_discrete_ui_choices() {
        let mut settings = Settings::default();
        settings.audio.output_sample_rate = 88_200;
        settings.ui.ui_scale = 0.6;
        settings.tilt.sensitivity = 8.0;
        settings.tilt.deadzone = 0.75;
        settings.emulation.uncapped_frames_per_tick = 200;
        settings.rewind.seconds = 90;
        settings.save_input_profile("High sensitivity").unwrap();

        validate(&settings).unwrap();
    }
}
