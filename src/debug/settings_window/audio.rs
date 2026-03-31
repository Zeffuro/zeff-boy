use crate::debug::ui_helpers::enum_combo_box;
use crate::settings::Settings;

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    const OUTPUT_SAMPLE_RATES: [u32; 5] = [32_000, 44_100, 48_000, 96_000, 192_000];

    ui.heading("Volume");
    ui.add(
        egui::Slider::new(&mut settings.audio.volume, 0.0..=1.0)
            .text("Master volume")
            .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
    );
    ui.checkbox(
        &mut settings.audio.mute_during_fast_forward,
        "Mute audio while fast-forward is held",
    );

    ui.separator();
    ui.heading("Output");
    egui::ComboBox::from_label("Sample rate")
        .selected_text(format!("{} Hz", settings.audio.output_sample_rate))
        .show_ui(ui, |ui| {
            for rate in OUTPUT_SAMPLE_RATES {
                ui.selectable_value(
                    &mut settings.audio.output_sample_rate,
                    rate,
                    format!("{rate} Hz"),
                );
            }
        });
    ui.label(
        egui::RichText::new("Uses the nearest rate supported by your current output device.")
            .weak()
            .small(),
    );

    ui.separator();
    ui.heading("Filtering");
    ui.checkbox(
        &mut settings.audio.low_pass_enabled,
        "Enable low-pass output filter",
    );
    ui.add_enabled_ui(settings.audio.low_pass_enabled, |ui| {
        ui.add(
            egui::Slider::new(&mut settings.audio.low_pass_cutoff_hz, 200..=12_000)
                .text("Cutoff")
                .custom_formatter(|value, _| format!("{value:.0} Hz")),
        );
    });
    ui.label(
        egui::RichText::new("Lower cutoff removes more high-frequency noise but dulls treble.")
            .weak()
            .small(),
    );

    ui.separator();
    ui.heading("Recording");

    enum_combo_box(ui, "Recording format", &mut settings.audio.recording_format);
    ui.label(
        egui::RichText::new(
            "16-bit PCM: smaller files, standard compatibility.\n\
             32-bit Float: lossless sample precision, ideal for editing.\n\
             OGG Vorbis: compressed lossy format, much smaller files.\n\
             MIDI: records APU channel notes/volumes as a Standard MIDI File.",
        )
        .weak()
        .small(),
    );
}
