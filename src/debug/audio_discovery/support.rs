use zeff_emu_common::system::System;

pub(super) fn draw(context: &egui::Context, open: &mut bool) {
    egui::Window::new("Supported audio")
        .open(open)
        .default_width(740.0)
        .default_height(480.0)
        .resizable(true)
        .show(context, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                ui.label("Audio Explorer support depends on the game's sound engine.");
                ui.small("These are extraction and independent playback capabilities. Game playback uses the emulator's own audio support.");
                ui.add_space(8.0);
                egui::Grid::new("audio-support-systems")
                    .striped(true)
                    .max_col_width(240.0)
                    .spacing([16.0, 10.0])
                    .show(ui, |ui| {
                        ui.strong("System");
                        ui.strong("Native audio discovery");
                        ui.strong("Preview / exports");
                        ui.end_row();
                        for spec in System::specs() {
                            let (name, engine, capability) = system_row(spec.system);
                            ui.label(name);
                            ui.label(engine);
                            ui.label(capability);
                            ui.end_row();
                        }
                    });
                ui.separator();
                ui.label("All cartridge systems: embedded XM, MOD, S3M and IT detection and original-file export. This does not cover their native sound drivers.");
                ui.label("Open audio file: XM, MOD, S3M, IT, VGM/VGZ, GBS v1, NSF v1 and NSFe inspection and preservation. Independent playback is not available for these files yet.");
                ui.small("MP2k preview is an approximate synth. Custom synthesis and runtime-dependent voices can differ from game playback. Available exports are checked for each selected song.");
                ui.small("FDS native discovery, PC Engine PSG/ADPCM discovery, and unlisted driver variants remain unsupported.");
            });
        });
}

fn system_row(system: System) -> (&'static str, &'static str, &'static str) {
    match system {
        System::Gb => (
            "GB / GBC / SGB",
            "Banked GB drivers, GHX, GB Sound System, Carillon and selected native profiles",
            "Eligible songs: original-driver preview / audio / MIDI; mapped data otherwise",
        ),
        System::Gba => (
            "Game Boy Advance",
            "MP2k / M4A, GAX, Natsume and selected native engine profiles",
            "Per song: approximate or original-driver preview / audio, MIDI / banks and mapped data when eligible",
        ),
        System::Nes => (
            "NES / FDS",
            "Exact NTSC queue and native-driver profiles; FDS unsupported",
            "Eligible songs: original-driver preview / audio; queue profiles can provide MIDI and mapped data",
        ),
        System::Pce => (
            "PC Engine / CD / SuperGrafx",
            "CD audio tracks; no cartridge driver",
            "CDDA preview; WAV / FLAC / Ogg",
        ),
        System::Coleco => (
            "ColecoVision",
            "No native driver yet",
            "Embedded modules only",
        ),
        System::Ws => (
            "WonderSwan / Color",
            "Qualified TOSE-style eight-slot driver profiles",
            "Original-driver preview / WAV / FLAC / Ogg and mapped data; no WSR or MIDI export",
        ),
        System::Sms => (
            "Master System",
            "Exact PSG driver profiles",
            "Eligible songs: original-driver preview / audio and mapped data",
        ),
        System::Gg => (
            "Game Gear",
            "Exact PSG driver profiles",
            "Eligible songs: original-driver preview / audio and mapped data",
        ),
        System::Sg => ("SG-1000", "No native driver yet", "Embedded modules only"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_rows_describe_available_native_profiles_without_fixed_counts() {
        for system in [System::Gb, System::Nes, System::Sms, System::Gg, System::Ws] {
            let (_, discovery, playback) = system_row(system);
            assert!(discovery.contains("profile"));
            assert!(playback.contains("preview"));
            assert!(!playback.starts_with("No preview"));
        }
        assert_eq!(system_row(System::Sg).1, "No native driver yet");
    }
}
