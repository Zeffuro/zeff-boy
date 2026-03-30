use crate::debug::ui_helpers::enum_combo_box;
use crate::settings::Settings;

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
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
