use crate::audio::DEFAULT_AUDIO_SAMPLE_RATE;
use crate::debug::ui_helpers::EnumLabel;
use crate::emu_thread::{EmuCommand, TasControlCommandKind};

use super::App;

#[derive(Clone, Copy)]
enum AudioFinalizeMode {
    User,
    Teardown,
}

fn should_finalize_audio(mode: AudioFinalizeMode, synchronized: bool) -> bool {
    synchronized || matches!(mode, AudioFinalizeMode::Teardown)
}

fn capture_start_needs_rollback(synchronized: bool) -> bool {
    !synchronized
}

impl App {
    pub(in crate::app) fn start_audio_recording(&mut self) {
        if !self.core_supports_audio() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.toast_manager
                .error("Audio recording is not available on web");
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Err(error) =
                self.preflight_emu_command_kind(TasControlCommandKind::AudioOrTimingConfiguration)
            {
                self.toast_manager.error(error.to_string());
                return;
            }
            let sample_rate = self
                .audio
                .as_ref()
                .map(|a| a.sample_rate())
                .unwrap_or(DEFAULT_AUDIO_SAMPLE_RATE);

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

            self.pause_for_dialog();
            let file = crate::platform::FileDialog::new()
                .set_title("Save Audio Recording")
                .set_directory(self.state_dialog_dir())
                .add_filter(format.label(), &[ext])
                .set_file_name(&default_name)
                .save_file();

            self.resume_after_dialog();
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
                    let synchronized = self.synchronize_audio_recording_capture(
                        crate::emu_thread::AudioRecordingCapture {
                            active: true,
                            semantic: captures_semantics,
                        },
                    );
                    if capture_start_needs_rollback(synchronized) {
                        self.rollback_audio_capture_start();
                        if let Err(error) = recorder.finish() {
                            log::warn!("Failed to finalize rejected audio recording: {error}");
                        }
                        self.toast_manager
                            .error("Could not start audio capture in the emulator");
                        return;
                    }
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
    }

    pub(in crate::app) fn stop_audio_recording(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Err(error) =
                self.preflight_emu_command_kind(TasControlCommandKind::AudioOrTimingConfiguration)
            {
                self.toast_manager.error(error.to_string());
                return;
            }
            let synchronized = self.synchronize_audio_recording_capture(
                crate::emu_thread::AudioRecordingCapture::default(),
            );
            if !should_finalize_audio(AudioFinalizeMode::User, synchronized) {
                self.toast_manager.error(
                    "Audio recording remains active because capture could not be synchronized",
                );
                return;
            }
        }
        self.finish_audio_recording();
    }

    pub(in crate::app) fn finish_audio_recording_for_teardown(&mut self) {
        if should_finalize_audio(AudioFinalizeMode::Teardown, false) {
            self.finish_audio_recording();
        }
    }

    fn finish_audio_recording(&mut self) {
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
    fn rollback_audio_capture_start(&mut self) {
        let command = EmuCommand::SetAudioRecordingCapture {
            capture: crate::emu_thread::AudioRecordingCapture::default(),
            acknowledged: None,
        };
        let sent = self
            .emu_thread
            .as_ref()
            .is_some_and(|thread| thread.send_checked(command));
        if !sent {
            self.terminalize_tas_control_command_loss();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn synchronize_audio_recording_capture(
        &mut self,
        capture: crate::emu_thread::AudioRecordingCapture,
    ) -> bool {
        let (acknowledged_tx, acknowledged_rx) = std::sync::mpsc::channel();
        let command = EmuCommand::SetAudioRecordingCapture {
            capture,
            acknowledged: Some(acknowledged_tx),
        };
        if self.send_emu_command_checked(command).is_err() {
            return false;
        }

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let synchronized = loop {
            while let Some(result) = self
                .emu_thread
                .as_ref()
                .and_then(crate::emu_thread::EmuThread::try_recv_frame)
            {
                self.process_frame_result(result);
            }
            match audio_capture_acknowledgement(acknowledged_rx.try_recv()) {
                Some(synchronized) => break synchronized,
                None if std::time::Instant::now() < deadline => {
                    std::thread::yield_now();
                }
                None => {
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

#[cfg(not(target_arch = "wasm32"))]
fn audio_capture_acknowledgement(
    result: Result<(), std::sync::mpsc::TryRecvError>,
) -> Option<bool> {
    match result {
        Ok(()) => Some(true),
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(false),
        Err(std::sync::mpsc::TryRecvError::Empty) => None,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn disconnected_audio_capture_ack_is_failure() {
        assert_eq!(
            audio_capture_acknowledgement(Err(std::sync::mpsc::TryRecvError::Disconnected)),
            Some(false)
        );
        assert_eq!(audio_capture_acknowledgement(Ok(())), Some(true));
        assert_eq!(
            audio_capture_acknowledgement(Err(std::sync::mpsc::TryRecvError::Empty)),
            None
        );
    }

    #[test]
    fn teardown_finalizes_even_when_worker_synchronization_fails() {
        assert!(!should_finalize_audio(AudioFinalizeMode::User, false));
        assert!(should_finalize_audio(AudioFinalizeMode::User, true));
        assert!(should_finalize_audio(AudioFinalizeMode::Teardown, false));
        assert!(capture_start_needs_rollback(false));
        assert!(!capture_start_needs_rollback(true));
    }
}
