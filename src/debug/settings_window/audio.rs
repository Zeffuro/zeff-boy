use crate::settings::Settings;

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Volume");
    ui.add(
        egui::Slider::new(&mut settings.master_volume, 0.0..=1.0)
            .text("Master volume")
            .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
    );
    ui.checkbox(
        &mut settings.mute_audio_during_fast_forward,
        "Mute audio while fast-forward is held",
    );

    ui.separator();
    ui.heading("Recording");

    use crate::settings::AudioRecordingFormat;
    egui::ComboBox::from_label("Recording format")
        .selected_text(settings.audio_recording_format.label())
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut settings.audio_recording_format,
                AudioRecordingFormat::Wav16,
                AudioRecordingFormat::Wav16.label(),
            );
            ui.selectable_value(
                &mut settings.audio_recording_format,
                AudioRecordingFormat::WavFloat,
                AudioRecordingFormat::WavFloat.label(),
            );
            ui.selectable_value(
                &mut settings.audio_recording_format,
                AudioRecordingFormat::OggVorbis,
                AudioRecordingFormat::OggVorbis.label(),
            );
            ui.selectable_value(
                &mut settings.audio_recording_format,
                AudioRecordingFormat::Midi,
                AudioRecordingFormat::Midi.label(),
            );
        });
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

