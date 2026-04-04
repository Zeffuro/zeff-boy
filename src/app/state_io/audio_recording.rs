use crate::debug::ui_helpers::EnumLabel;

use super::App;

impl App {
    pub(in crate::app) fn start_audio_recording(&mut self) {
        let sample_rate = self
            .audio
            .as_ref()
            .map(|a| a.sample_rate())
            .unwrap_or(48_000);

        let format = self.settings.audio.recording_format;
        let ext = format.extension();

        let default_name = self
            .rom_info
            .rom_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|stem| format!("{stem}.{ext}"))
            .unwrap_or_else(|| format!("recording.{ext}"));

        let was_paused = self.pause_for_dialog();
        let file = rfd::FileDialog::new()
            .set_title("Save Audio Recording")
            .set_directory(self.state_dialog_dir())
            .add_filter(format.label(), &[ext])
            .set_file_name(&default_name)
            .save_file();

        self.resume_after_dialog(was_paused);
        let Some(path) = file else {
            return;
        };

        match crate::audio_recorder::AudioRecorder::start(&path, sample_rate, format) {
            Ok(recorder) => {
                log::info!("Started audio recording to {}", path.display());
                self.toast_manager.info("Recording audio...");
                self.recording.audio_recorder = Some(recorder);
            }
            Err(err) => {
                log::error!("Failed to start recording: {}", err);
                self.toast_manager.error(format!("Record failed: {err}"));
            }
        }
    }

    pub(in crate::app) fn stop_audio_recording(&mut self) {
        if let Some(recorder) = self.recording.audio_recorder.take() {
            match recorder.finish() {
                Ok(path) => {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                    log::info!("Audio saved to {}", path.display());
                    self.toast_manager.success(format!("Saved {name}"));
                }
                Err(err) => {
                    log::error!("Failed to finalize recording: {}", err);
                    self.toast_manager.error(format!("Recording error: {err}"));
                }
            }
        }
    }
}
