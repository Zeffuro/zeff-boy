use crossbeam_channel::{Sender, TrySendError};

use crate::emu_backend::EmuBackend;
use crate::link::RemoteLink;
use crate::link::transport::TcpLinkTransport;
use crate::ui;

use super::AudioRecordingCapture;
use super::types::publish_framebuffer;
use super::{EmuThread, FrameResult, SharedFramebuffer, WorkerRuntimeFault};

impl EmuThread {
    pub(crate) fn frame_requires_preserved_delivery(
        result: &FrameResult,
        recording_active: bool,
    ) -> bool {
        recording_active
            || !result.game_boy_printer_jobs.is_empty()
            || result.runtime_fault.is_some()
    }

    pub(crate) fn send_frame(
        frame_tx: &Sender<FrameResult>,
        drain_rx: &crossbeam_channel::Receiver<FrameResult>,
        result: FrameResult,
        preserve_delivery: bool,
    ) -> bool {
        if preserve_delivery {
            return frame_tx.send(result).is_ok();
        }
        match frame_tx.try_send(result) {
            Ok(()) => true,
            Err(TrySendError::Full(result)) => {
                let mut queued = Vec::new();
                while let Ok(frame) = drain_rx.try_recv() {
                    queued.push(frame);
                }
                if queued.iter().any(|frame| frame.delivery_merged) {
                    for frame in queued {
                        if frame_tx.send(frame).is_err() {
                            return false;
                        }
                    }
                    return frame_tx.send(result).is_ok();
                }
                let Some(mut merged) = queued.pop() else {
                    return frame_tx.send(result).is_ok();
                };
                for frame in queued.into_iter().rev() {
                    merged = Self::merge_replaced_frame(frame, merged);
                }
                frame_tx
                    .send(Self::merge_replaced_frame(merged, result))
                    .is_ok()
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    fn merge_replaced_frame(
        mut replaced: FrameResult,
        mut replacement: FrameResult,
    ) -> FrameResult {
        replaced
            .replay_events
            .append(&mut replacement.replay_events);
        replacement.replay_events = replaced.replay_events;
        if replaced.audio_playback_speed == replacement.audio_playback_speed {
            replaced
                .audio_samples
                .append(&mut replacement.audio_samples);
            replacement.audio_samples = replaced.audio_samples;
        }
        replaced
            .audio_semantic_frames
            .append(&mut replacement.audio_semantic_frames);
        replacement.audio_semantic_frames = replaced.audio_semantic_frames;
        replaced
            .audio_timeline_discontinuities
            .append(&mut replacement.audio_timeline_discontinuities);
        replacement.audio_timeline_discontinuities = replaced.audio_timeline_discontinuities;
        replaced
            .game_boy_printer_jobs
            .append(&mut replacement.game_boy_printer_jobs);
        replacement.game_boy_printer_jobs = replaced.game_boy_printer_jobs;
        replacement.advanced_frames = replacement
            .advanced_frames
            .saturating_add(replaced.advanced_frames);
        replacement.delivery_merged = true;
        if replacement.replay_error.is_none() {
            replacement.replay_error = replaced.replay_error;
        }
        if replacement.runtime_fault.is_none() {
            replacement.runtime_fault = replaced.runtime_fault;
        }
        replacement
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_uncapped_batch(
        backend: &mut EmuBackend,
        cheats: &[crate::cheats::CheatPatch],
        tcp_link: Option<&mut RemoteLink<TcpLinkTransport>>,
        shared_fb: &SharedFramebuffer,
        rewind_buffer: &zeff_emu_common::rewind::RewindBuffer,
        frame_tx: &Sender<FrameResult>,
        drain_rx: &crossbeam_channel::Receiver<FrameResult>,
        recording_capture: AudioRecordingCapture,
        pending_discontinuities: &mut Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
        runtime_fault: &mut WorkerRuntimeFault,
        batch_size: usize,
    ) {
        runtime_fault.latch(backend.take_runtime_fault());
        if !runtime_fault.can_step() {
            if let Some(fault) = runtime_fault.take_pending_delivery() {
                publish_framebuffer(shared_fb, backend.framebuffer());
                let result = Self::build_uncapped_frame_result(
                    backend,
                    rewind_buffer,
                    0,
                    Vec::new(),
                    Some(fault),
                );
                Self::send_frame(frame_tx, drain_rx, result, true);
            }
            std::thread::yield_now();
            return;
        }
        if backend.is_suspended() {
            std::thread::yield_now();
            return;
        }

        let mut audio_semantic_frames = Vec::new();
        let advanced_frames = if tcp_link.is_some() {
            Self::step_n_frames_with_tcp_link_and_runtime_fault(
                backend,
                batch_size,
                cheats,
                tcp_link,
                recording_capture.semantic,
                &mut audio_semantic_frames,
                None,
                runtime_fault,
            )
        } else {
            Self::step_n_frames_with_runtime_fault(
                backend,
                batch_size,
                cheats,
                recording_capture.semantic,
                &mut audio_semantic_frames,
                None,
                runtime_fault,
            )
        };

        publish_framebuffer(shared_fb, backend.framebuffer());

        let mut result = Self::build_uncapped_frame_result(
            backend,
            rewind_buffer,
            advanced_frames,
            audio_semantic_frames,
            runtime_fault.take_pending_delivery(),
        );
        if !result.audio_semantic_frames.is_empty() {
            result
                .audio_timeline_discontinuities
                .append(pending_discontinuities);
        }

        let preserve_delivery =
            Self::frame_requires_preserved_delivery(&result, recording_capture.active);
        Self::send_frame(frame_tx, drain_rx, result, preserve_delivery);

        std::thread::yield_now();
    }

    fn build_uncapped_frame_result(
        backend: &mut EmuBackend,
        rewind_buffer: &zeff_emu_common::rewind::RewindBuffer,
        advanced_frames: usize,
        audio_semantic_frames: Vec<crate::audio_tooling::AudioSemanticFrame>,
        runtime_fault: Option<String>,
    ) -> FrameResult {
        FrameResult {
            advanced_frames,
            delivery_merged: false,
            replay_events: Vec::new(),
            replay_error: None,
            runtime_fault,
            rumble: backend.rumble_active(),
            audio_samples: Vec::new(),
            audio_playback_speed: 1,
            ui_data: ui::UiFrameData::default(),
            is_mbc7: backend.is_mbc7(),
            is_gba_tilt: backend.is_gba_tilt(),
            is_pocket_camera: backend.is_pocket_camera(),
            game_boy_serial_device: backend.game_boy_serial_device(),
            game_boy_printer_jobs: Self::take_game_boy_printer_jobs(backend),
            media_slot_snapshot: backend.media_slot_snapshot(),
            rewind_fill: rewind_buffer.fill_ratio(),
            audio_semantic_frames,
            audio_timeline_discontinuities: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn empty_result() -> FrameResult {
        FrameResult {
            advanced_frames: 0,
            delivery_merged: false,
            replay_events: Vec::new(),
            replay_error: None,
            runtime_fault: None,
            rumble: false,
            audio_samples: Vec::new(),
            audio_playback_speed: 1,
            ui_data: ui::UiFrameData::default(),
            is_mbc7: false,
            is_gba_tilt: false,
            is_pocket_camera: false,
            game_boy_serial_device: None,
            game_boy_printer_jobs: Vec::new(),
            media_slot_snapshot: None,
            rewind_fill: 0.0,
            audio_semantic_frames: Vec::new(),
            audio_timeline_discontinuities: Vec::new(),
        }
    }

    #[test]
    fn preserved_frame_delivery_waits_for_channel_capacity_without_dropping() {
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(1);
        frame_tx.send(empty_result()).unwrap();
        let drain_rx = frame_rx.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();

        let sender = std::thread::spawn(move || {
            let sent = EmuThread::send_frame(&frame_tx, &drain_rx, empty_result(), true);
            done_tx.send(sent).unwrap();
        });

        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_millis(20))
                .is_err()
        );
        assert!(frame_rx.recv().is_ok());
        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap()
        );
        assert!(frame_rx.recv().is_ok());
        sender.join().unwrap();
    }

    #[test]
    fn replaceable_frames_carry_forward_completed_printer_jobs() {
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(1);
        let drain_rx = frame_rx.clone();
        let mut completed = empty_result();
        completed
            .game_boy_printer_jobs
            .push(zeff_gb_core::hardware::GameBoyPrinterJob {
                pixels: vec![1; zeff_gb_core::hardware::GAME_BOY_PRINTER_WIDTH * 8],
                height: 8,
                copies: 1,
                feed_before: 0,
                feed_after: 0,
                palette: 0xE4,
                density: 0x40,
            });
        frame_tx.send(completed).unwrap();

        assert!(EmuThread::send_frame(
            &frame_tx,
            &drain_rx,
            empty_result(),
            false
        ));

        let delivered = frame_rx.recv().unwrap();
        assert_eq!(delivered.game_boy_printer_jobs.len(), 1);
        assert_eq!(delivered.game_boy_printer_jobs[0].height, 8);
    }

    #[test]
    fn replaceable_frames_preserve_ordered_audio_and_semantic_payloads() {
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(1);
        let drain_rx = frame_rx.clone();
        let mut older = empty_result();
        older.advanced_frames = 2;
        older.audio_samples = vec![1.0, 2.0];
        older
            .audio_semantic_frames
            .push(crate::audio_tooling::AudioSemanticFrame {
                frame: 2,
                tempo_us_per_beat: 1,
                voices: Vec::new(),
            });
        older
            .audio_timeline_discontinuities
            .push(crate::audio_recorder::AudioTimelineDiscontinuity::Rewind);
        frame_tx.send(older).unwrap();

        let mut newer = empty_result();
        newer.advanced_frames = 3;
        newer.audio_samples = vec![3.0, 4.0];
        newer
            .audio_semantic_frames
            .push(crate::audio_tooling::AudioSemanticFrame {
                frame: 3,
                tempo_us_per_beat: 1,
                voices: Vec::new(),
            });
        newer
            .audio_timeline_discontinuities
            .push(crate::audio_recorder::AudioTimelineDiscontinuity::Reset);

        assert!(EmuThread::send_frame(&frame_tx, &drain_rx, newer, false));
        let delivered = frame_rx.recv().unwrap();
        assert_eq!(delivered.advanced_frames, 5);
        assert_eq!(delivered.audio_samples, vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(
            delivered
                .audio_semantic_frames
                .iter()
                .map(|frame| frame.frame)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(
            delivered.audio_timeline_discontinuities,
            vec![
                crate::audio_recorder::AudioTimelineDiscontinuity::Rewind,
                crate::audio_recorder::AudioTimelineDiscontinuity::Reset,
            ]
        );
    }

    #[test]
    fn replacement_drops_audio_from_a_different_playback_speed() {
        let mut older = empty_result();
        older.audio_samples = vec![1.0, 2.0];
        older.audio_playback_speed = 4;
        let mut newer = empty_result();
        newer.audio_samples = vec![3.0, 4.0];

        let merged = EmuThread::merge_replaced_frame(older, newer);

        assert_eq!(merged.audio_playback_speed, 1);
        assert_eq!(merged.audio_samples, vec![3.0, 4.0]);
    }

    #[test]
    fn repeated_replacement_applies_bounded_backpressure() {
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
        let drain_rx = frame_rx.clone();
        frame_tx.send(empty_result()).unwrap();
        frame_tx.send(empty_result()).unwrap();
        assert!(EmuThread::send_frame(
            &frame_tx,
            &drain_rx,
            empty_result(),
            false
        ));
        assert!(EmuThread::send_frame(
            &frame_tx,
            &drain_rx,
            empty_result(),
            false
        ));

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let sender = std::thread::spawn(move || {
            let mut result = empty_result();
            result.audio_samples = vec![1.0; 4];
            done_tx
                .send(EmuThread::send_frame(&frame_tx, &drain_rx, result, false))
                .unwrap();
        });
        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_millis(20))
                .is_err()
        );
        assert!(frame_rx.recv().is_ok());
        assert!(frame_rx.recv().is_ok());
        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap()
        );
        sender.join().unwrap();
    }

    #[test]
    fn runtime_fault_frames_require_preserved_delivery() {
        let mut result = empty_result();
        assert!(!EmuThread::frame_requires_preserved_delivery(
            &result, false
        ));
        result.runtime_fault = Some("machine fault".to_string());
        assert!(EmuThread::frame_requires_preserved_delivery(&result, false));
    }

    #[test]
    fn replaceable_frames_carry_forward_runtime_faults() {
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(1);
        let drain_rx = frame_rx.clone();
        let mut faulted = empty_result();
        faulted.runtime_fault = Some("machine fault".to_string());
        frame_tx.send(faulted).unwrap();

        assert!(EmuThread::send_frame(
            &frame_tx,
            &drain_rx,
            empty_result(),
            false
        ));
        assert_eq!(
            frame_rx.recv().unwrap().runtime_fault.as_deref(),
            Some("machine fault")
        );
    }

    #[test]
    fn uncapped_semantic_capture_preserves_every_advanced_frame() {
        let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
            &[0x00],
            44_100,
            zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        )
        .unwrap();
        let mut backend = EmuBackend::from_sega8(emu, PathBuf::from("test.sms"));
        let shared_fb = super::super::types::new_shared_framebuffer();
        let rewind = zeff_emu_common::rewind::RewindBuffer::new(10, 4);
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
        let drain_rx = frame_rx.clone();
        let mut discontinuities = Vec::new();
        let mut runtime_fault = WorkerRuntimeFault::default();

        EmuThread::run_uncapped_batch(
            &mut backend,
            &[],
            None,
            &shared_fb,
            &rewind,
            &frame_tx,
            &drain_rx,
            AudioRecordingCapture {
                active: true,
                semantic: true,
            },
            &mut discontinuities,
            &mut runtime_fault,
            7,
        );

        let result = frame_rx.recv().unwrap();
        assert_eq!(result.advanced_frames, 7);
        assert_eq!(result.audio_semantic_frames.len(), 7);
    }

    #[test]
    fn uncapped_batch_does_not_reenter_a_faulted_worker() {
        let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
            &[0x00],
            44_100,
            zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        )
        .unwrap();
        let mut backend = EmuBackend::from_sega8(emu, PathBuf::from("test.sms"));
        let frame_before = backend.frame_count();
        let shared_fb = super::super::types::new_shared_framebuffer();
        let rewind = zeff_emu_common::rewind::RewindBuffer::new(10, 4);
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
        let drain_rx = frame_rx.clone();
        let mut discontinuities = Vec::new();
        let mut runtime_fault = WorkerRuntimeFault::default();
        runtime_fault.latch(Some("fault".to_string()));

        EmuThread::run_uncapped_batch(
            &mut backend,
            &[],
            None,
            &shared_fb,
            &rewind,
            &frame_tx,
            &drain_rx,
            AudioRecordingCapture::default(),
            &mut discontinuities,
            &mut runtime_fault,
            3,
        );

        assert_eq!(backend.frame_count(), frame_before);
        assert_eq!(
            frame_rx.recv().unwrap().runtime_fault.as_deref(),
            Some("fault")
        );

        EmuThread::run_uncapped_batch(
            &mut backend,
            &[],
            None,
            &shared_fb,
            &rewind,
            &frame_tx,
            &drain_rx,
            AudioRecordingCapture::default(),
            &mut discontinuities,
            &mut runtime_fault,
            3,
        );
        assert!(frame_rx.try_recv().is_err());
    }
}
