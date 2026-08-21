use crossbeam_channel::{Sender, TrySendError};

use crate::emu_backend::EmuBackend;
use crate::link::RemoteLink;
use crate::link::transport::TcpLinkTransport;
use crate::ui;

use super::AudioRecordingCapture;
use super::types::publish_framebuffer;
use super::{EmuThread, FrameResult, SharedFramebuffer};

const UNCAPPED_BATCH_SIZE: usize = 60;

impl EmuThread {
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
            Err(TrySendError::Full(mut result)) => {
                if let Ok(mut replaced) = drain_rx.try_recv()
                    && !replaced.game_boy_printer_images.is_empty()
                {
                    replaced
                        .game_boy_printer_images
                        .append(&mut result.game_boy_printer_images);
                    result.game_boy_printer_images = replaced.game_boy_printer_images;
                }
                frame_tx.try_send(result).is_ok()
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
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
    ) {
        if backend.is_suspended() {
            std::thread::yield_now();
            return;
        }

        let mut audio_semantic_frames = Vec::new();
        let advanced_frames = if tcp_link.is_some() {
            Self::step_n_frames_with_tcp_link(
                backend,
                UNCAPPED_BATCH_SIZE,
                cheats,
                tcp_link,
                recording_capture.semantic,
                &mut audio_semantic_frames,
                None,
            )
        } else {
            Self::step_n_frames(
                backend,
                UNCAPPED_BATCH_SIZE,
                cheats,
                recording_capture.semantic,
                &mut audio_semantic_frames,
                None,
            )
        };

        publish_framebuffer(shared_fb, backend.framebuffer());

        let mut result = FrameResult {
            advanced_frames,
            replay_events: Vec::new(),
            replay_error: None,
            rumble: backend.rumble_active(),
            audio_samples: Vec::new(),
            ui_data: ui::UiFrameData::default(),
            is_mbc7: backend.is_mbc7(),
            is_pocket_camera: backend.is_pocket_camera(),
            game_boy_serial_device: backend.game_boy_serial_device(),
            game_boy_printer_images: Self::take_game_boy_printer_images(backend),
            media_slot_snapshot: backend.media_slot_snapshot(),
            rewind_fill: rewind_buffer.fill_ratio(),
            audio_semantic_frames,
            audio_timeline_discontinuities: Vec::new(),
        };
        if !result.audio_semantic_frames.is_empty() {
            result
                .audio_timeline_discontinuities
                .append(pending_discontinuities);
        }

        let preserve_delivery =
            recording_capture.active || !result.game_boy_printer_images.is_empty();
        Self::send_frame(frame_tx, drain_rx, result, preserve_delivery);

        std::thread::yield_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn empty_result() -> FrameResult {
        FrameResult {
            advanced_frames: 0,
            replay_events: Vec::new(),
            replay_error: None,
            rumble: false,
            audio_samples: Vec::new(),
            ui_data: ui::UiFrameData::default(),
            is_mbc7: false,
            is_pocket_camera: false,
            game_boy_serial_device: None,
            game_boy_printer_images: Vec::new(),
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
    fn replaceable_frames_carry_forward_completed_printer_images() {
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(1);
        let drain_rx = frame_rx.clone();
        let mut completed = empty_result();
        completed
            .game_boy_printer_images
            .push(super::super::GameBoyPrinterImage {
                width: 1,
                height: 1,
                rgba: vec![1, 2, 3, 4],
            });
        frame_tx.send(completed).unwrap();

        assert!(EmuThread::send_frame(
            &frame_tx,
            &drain_rx,
            empty_result(),
            false
        ));

        let delivered = frame_rx.recv().unwrap();
        assert_eq!(delivered.game_boy_printer_images.len(), 1);
        assert_eq!(delivered.game_boy_printer_images[0].rgba, [1, 2, 3, 4]);
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
        );

        let result = frame_rx.recv().unwrap();
        assert_eq!(result.advanced_frames, UNCAPPED_BATCH_SIZE);
        assert_eq!(result.audio_semantic_frames.len(), UNCAPPED_BATCH_SIZE);
    }
}
