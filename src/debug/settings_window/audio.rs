use crate::audio_recorder::ogg_vorbis_supported;
#[cfg(not(target_arch = "wasm32"))]
use crate::debug::ui_helpers::EnumLabel;
use crate::settings::Settings;

use super::{
    SettingsUiState,
    layout::{checkbox, enum_combo, helper, row},
    search::{self, SettingId as Id},
};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, settings_ui: &mut SettingsUiState) {
    const OUTPUT_SAMPLE_RATES: [u32; 5] = [32_000, 44_100, 48_000, 96_000, 192_000];

    helper(
        ui,
        egui::RichText::new("Output, filtering, and recording defaults.")
            .small()
            .weak(),
    );
    ui.add_space(14.0);
    ui.heading("Volume");
    row(ui, Id::AudioMasterVolume, None, |ui| {
        ui.add_sized(
            [240.0, 30.0],
            egui::Slider::new(&mut settings.audio.volume, 0.0..=1.0)
                .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
        )
    });
    checkbox(
        ui,
        Id::AudioMuteAudioWhileFastForwardIsHeld,
        None,
        &mut settings.audio.mute_during_fast_forward,
    );

    ui.add_space(22.0);
    ui.heading("Output device & buffering");
    draw_host_output(ui, settings, settings_ui);

    ui.add_space(22.0);
    ui.heading("Output format");
    row(ui, Id::AudioEmulatorSampleRate, None, |ui| {
        egui::ComboBox::from_id_salt("audio_sample_rate")
            .selected_text(format!("{} Hz", settings.audio.output_sample_rate))
            .width(220.0)
            .show_ui(ui, |ui| {
                for rate in OUTPUT_SAMPLE_RATES {
                    ui.selectable_value(
                        &mut settings.audio.output_sample_rate,
                        rate,
                        format!("{rate} Hz"),
                    );
                }
            })
            .response
            .on_hover_text(
                "Your audio device may use a different rate; output is resampled automatically.",
            )
    });

    ui.add_space(22.0);
    ui.heading("Output filter");
    checkbox(
        ui,
        Id::AudioEnableLowPassOutputFilter,
        None,
        &mut settings.audio.low_pass_enabled,
    );
    ui.indent("audio_low_pass_cutoff", |ui| {
        search::conditional(
            ui,
            Id::AudioLowPassCutoff,
            Id::AudioEnableLowPassOutputFilter,
            settings.audio.low_pass_enabled,
            "Enable the low-pass output filter to edit the cutoff.",
        );
        row(ui, Id::AudioLowPassCutoff, None, |ui| {
            ui.add_enabled(
                settings.audio.low_pass_enabled,
                egui::Slider::new(&mut settings.audio.low_pass_cutoff_hz, 200..=12_000)
                    .custom_formatter(|value, _| format!("{value:.0} Hz")),
            )
        });
    });

    ui.add_space(22.0);
    ui.heading("Audio recording");
    enum_combo(
        ui,
        Id::AudioRecordingFormat,
        "audio_recording_format",
        None,
        &mut settings.audio.recording_format,
    );
    if !ogg_vorbis_supported() {
        helper(
            ui,
            egui::RichText::new(
                "OGG Vorbis unavailable in this build (requires `audio-recording` feature).",
            )
            .small()
            .weak(),
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_host_output(ui: &mut egui::Ui, settings: &mut Settings, settings_ui: &mut SettingsUiState) {
    let devices = crate::audio::output_devices();
    let unavailable = devices.as_ref().ok().is_some_and(|devices| {
        settings
            .audio
            .output_device_id
            .as_ref()
            .is_some_and(|selected| !devices.iter().any(|device| &device.id == selected))
    });
    let selected_text = match (&settings.audio.output_device_id, unavailable) {
        (None, _) => "System default".to_owned(),
        (Some(_), true) => "Saved output unavailable".to_owned(),
        (Some(id), false) => devices
            .as_ref()
            .ok()
            .and_then(|devices| devices.iter().find(|device| &device.id == id))
            .map_or_else(|| "Saved output".to_owned(), |device| device.name.clone()),
    };

    row(ui, Id::AudioOutputDevice, None, |ui| {
        egui::ComboBox::from_id_salt("audio_output_device")
            .selected_text(selected_text)
            .width(220.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut settings.audio.output_device_id, None, "System default");
                if let Ok(devices) = &devices {
                    for device in devices {
                        ui.selectable_value(
                            &mut settings.audio.output_device_id,
                            Some(device.id.clone()),
                            &device.name,
                        )
                        .on_hover_text("Stable host device selection");
                    }
                }
            })
            .response
    });
    match devices {
        Ok(_) if unavailable => helper(
            ui,
            "The saved output is unavailable. Zeff-boy tries System default; the active output is shown below. Reconnect the device, then choose Retry audio output to use it again.",
        ),
        Err(error) => helper(
            ui,
            format!(
                "Could not list output devices ({error}). You can still select System default."
            ),
        ),
        Ok(_) => {}
    }
    if let Some(active) = &settings_ui.audio_host_status.active_device {
        helper(ui, format!("Active output: {}", active.name));
    }
    if let Some(reason) = &settings_ui.audio_host_status.device_fallback {
        helper(ui, egui::RichText::new(reason).color(egui::Color32::YELLOW));
    }
    enum_combo(
        ui,
        Id::AudioBufferPolicy,
        "audio_buffer_policy",
        None,
        &mut settings.audio.buffer_policy,
    );
    helper(
        ui,
        "Queue presets control Zeff's application queue, not the operating system or device hardware buffer. Low latency falls back to Auto after 3 underruns within 5 seconds.",
    );
    if let Some(fallback) = &settings_ui.audio_host_status.buffer_fallback {
        helper(
            ui,
            egui::RichText::new(format!(
                "{} underrun reports moved this run from {} to {} buffering. Your saved preference is unchanged.",
                fallback.underrun_reports,
                fallback.requested.label(),
                fallback.active.label(),
            ))
            .color(egui::Color32::YELLOW),
        );
    }
    if row(ui, Id::AudioRetryOutput, None, |ui| {
        ui.button("Retry audio output")
    })
    .clicked()
    {
        settings_ui.audio_retry_requested = true;
    }
}

#[cfg(target_arch = "wasm32")]
fn draw_host_output(
    ui: &mut egui::Ui,
    _settings: &mut Settings,
    _settings_ui: &mut SettingsUiState,
) {
    helper(
        ui,
        "Output device selection is browser-managed. Queue policy and native device controls are unavailable here.",
    );
}
