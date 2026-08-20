use crate::debug::ui_helpers::EnumLabel;

use super::App;

impl App {
    pub(in crate::app) fn start_audio_recording(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            self.toast_manager
                .error("Audio recording is not available on web");
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let sample_rate = self
                .audio
                .as_ref()
                .map(|a| a.sample_rate())
                .unwrap_or(48_000);

            let format = self.settings.audio.recording_format;
            let captures_semantics = format.captures_semantics();
            if self.timing.uncapped_speed && !format.supports_uncapped_recording() {
                self.toast_manager
                    .error("This recording format is unavailable in uncapped benchmark mode");
                return;
            }
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
            let file = crate::platform::FileDialog::new()
                .set_title("Save Audio Recording")
                .set_directory(self.state_dialog_dir())
                .add_filter(format.label(), &[ext])
                .set_file_name(&default_name)
                .save_file();

            self.resume_after_dialog(was_paused);
            let Some(path) = file else {
                return;
            };

            if !self.synchronize_audio_recording_capture(
                crate::emu_thread::AudioRecordingCapture::default(),
            ) {
                self.toast_manager
                    .error("Could not synchronize audio recording with the emulator");
                return;
            }

            let context = self
                .emu_thread
                .as_ref()
                .and_then(crate::emu_thread::EmuThread::audio_recording_context);
            match crate::audio_recorder::AudioRecorder::start(&path, sample_rate, format, context) {
                Ok(recorder) => {
                    log::info!("Started audio recording to {}", path.display());
                    self.toast_manager.info("Recording audio...");
                    self.recording.audio_recorder = Some(recorder);
                    if let Some(thread) = &self.emu_thread {
                        thread.send(crate::emu_thread::EmuCommand::SetAudioRecordingCapture {
                            capture: crate::emu_thread::AudioRecordingCapture {
                                active: true,
                                semantic: captures_semantics,
                            },
                            acknowledged: None,
                        });
                    }
                }
                Err(err) => {
                    log::error!("Failed to start recording: {}", err);
                    self.toast_manager.error(format!("Record failed: {err}"));
                }
            }
        }
    }

    pub(in crate::app) fn stop_audio_recording(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let synchronized = self.synchronize_audio_recording_capture(
                crate::emu_thread::AudioRecordingCapture::default(),
            );
            if !synchronized {
                self.recording.audio_recorder.take();
                self.toast_manager.error(
                    "Audio recording was aborted because pending frames could not be drained",
                );
                return;
            }
        }
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

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn synchronize_audio_recording_capture(
        &mut self,
        capture: crate::emu_thread::AudioRecordingCapture,
    ) -> bool {
        let Some(thread) = &self.emu_thread else {
            return true;
        };
        let (acknowledged_tx, acknowledged_rx) = std::sync::mpsc::channel();
        thread.send(crate::emu_thread::EmuCommand::SetAudioRecordingCapture {
            capture,
            acknowledged: Some(acknowledged_tx),
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let synchronized = loop {
            while let Some(result) = self
                .emu_thread
                .as_ref()
                .and_then(crate::emu_thread::EmuThread::try_recv_frame)
            {
                self.process_frame_result(result);
            }
            match acknowledged_rx.try_recv() {
                Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected) => break true,
                Err(std::sync::mpsc::TryRecvError::Empty)
                    if std::time::Instant::now() < deadline =>
                {
                    std::thread::yield_now();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    log::warn!("Timed out synchronizing audio recording capture");
                    break false;
                }
            }
        };
        while let Some(result) = self
            .emu_thread
            .as_ref()
            .and_then(crate::emu_thread::EmuThread::try_recv_frame)
        {
            self.process_frame_result(result);
        }
        synchronized
    }
}
