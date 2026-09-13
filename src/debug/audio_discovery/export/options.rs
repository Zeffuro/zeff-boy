use super::*;

pub(super) fn draw_native_options(ui: &mut egui::Ui, state: &mut ExportState) {
    ui.add_enabled_ui(!state.is_busy(), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("Duration (seconds)");
            ui.add(
                egui::DragValue::new(&mut state.native_options.max_seconds)
                    .range(1..=MAX_DURATION_SECONDS),
            );
            ui.label("End fade (seconds)");
            let max_fade = state
                .native_options
                .max_seconds
                .min(u16::from(MAX_FADE_SECONDS)) as u8;
            state.native_options.fade_seconds = state.native_options.fade_seconds.min(max_fade);
            ui.add(
                egui::DragValue::new(&mut state.native_options.fade_seconds).range(0..=max_fade),
            );
            egui::ComboBox::from_id_salt("audio-native-sample-rate")
                .selected_text(format!("{} Hz", state.native_options.sample_rate))
                .show_ui(ui, |ui| {
                    for rate in SAMPLE_RATES {
                        ui.selectable_value(
                            &mut state.native_options.sample_rate,
                            rate,
                            format!("{rate} Hz"),
                        );
                    }
                });
        });
    });
    ui.small("The fade is included in the duration. Song endings and loops are not detected automatically.");
}

pub(super) fn draw_song_options(ui: &mut egui::Ui, state: &mut ExportState) {
    egui::CollapsingHeader::new("Song export settings").show(ui, |ui| {
        ui.add_enabled_ui(!state.is_busy(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Loop passes");
                ui.add(egui::DragValue::new(&mut state.options.loops).range(1..=MAX_LOOP_PASSES));
                ui.label("Maximum seconds");
                ui.add(egui::DragValue::new(&mut state.options.max_seconds).range(1..=MAX_DURATION_SECONDS));
                ui.label("Fade seconds");
                ui.add(egui::DragValue::new(&mut state.options.fade_seconds).range(0..=MAX_FADE_SECONDS));
            });
            if matches!(state.song_format, SongFormat::Audio(_)) {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Output sample rate");
                    egui::ComboBox::from_id_salt("audio-render-sample-rate")
                        .selected_text(format!("{} Hz", state.options.sample_rate))
                        .show_ui(ui, |ui| {
                            for rate in SAMPLE_RATES {
                                ui.selectable_value(
                                    &mut state.options.sample_rate,
                                    rate,
                                    format!("{rate} Hz"),
                                );
                            }
                        });
                });
                ui.small("The output rate sets the rendered file's sample rate. Extracted samples retain their own rates.");
            }
            if state.song_format.is_gsf() {
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut state.options.skip_channel10, "Skip MIDI channel 10");
                ui.label("Playback gain");
                egui::ComboBox::from_id_salt("audio-playback-gain")
                    .selected_text(state.options.playback_gain.label())
                    .show_ui(ui, |ui| {
                        for gain in PlaybackGain::ALL {
                            ui.selectable_value(&mut state.options.playback_gain, gain, gain.label());
                        }
                    });
                ui.label("Bank select");
                egui::ComboBox::from_id_salt("audio-bank-select")
                    .selected_text(state.options.bank_select.label())
                    .show_ui(ui, |ui| {
                        for convention in BankSelect::ALL {
                            ui.selectable_value(
                                &mut state.options.bank_select,
                                convention,
                                convention.label(),
                            );
                        }
                    });
            });
        });
        if state.song_format.is_gsf() {
            ui.small("Loop passes estimates the playback length; maximum seconds caps length plus fade. Your GSF player applies these tags and chooses its output sample rate.");
            return;
        }
        ui.small("MP2k amplitude compensates for the MIDI synth response to note velocity and CC7 volume. Raw controls remain the default. This changes MIDI and rendered audio, not live game audio.");
        ui.small("Loop passes is the total number of sequence passes, so 1 plays the sequence once. MIDI and rendered audio use the loop and duration settings. Fade applies only to rendered audio. MIDI contains decoded sequence tracks; load the matching instrument bank for its programs. Skip channel 10 avoids the percussion convention and uses MIDI port 1 for a sixteenth source track.");
    });
}
