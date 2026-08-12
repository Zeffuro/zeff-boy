use crossbeam_channel::{Sender, TrySendError};

use crate::emu_backend::EmuBackend;
use crate::link::gb::GameBoyRemoteLink;
use crate::link::transport::TcpLinkTransport;
use crate::ui;

use super::types::publish_framebuffer;
use super::{EmuThread, FrameResult, SharedFramebuffer};

const UNCAPPED_BATCH_SIZE: usize = 60;

impl EmuThread {
    pub(crate) fn send_frame(
        frame_tx: &Sender<FrameResult>,
        drain_rx: &crossbeam_channel::Receiver<FrameResult>,
        result: FrameResult,
    ) -> bool {
        match frame_tx.try_send(result) {
            Ok(()) => true,
            Err(TrySendError::Full(result)) => {
                let _ = drain_rx.try_recv();
                frame_tx.try_send(result).is_ok()
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_uncapped_batch(
        backend: &mut EmuBackend,
        cheats: &[crate::cheats::CheatPatch],
        tcp_link: Option<&mut GameBoyRemoteLink<TcpLinkTransport>>,
        shared_fb: &SharedFramebuffer,
        rewind_buffer: &zeff_emu_common::rewind::RewindBuffer,
        frame_tx: &Sender<FrameResult>,
        drain_rx: &crossbeam_channel::Receiver<FrameResult>,
    ) {
        if backend.is_suspended() {
            std::thread::yield_now();
            return;
        }

        if tcp_link.is_some() {
            Self::step_n_frames_with_tcp_link(backend, UNCAPPED_BATCH_SIZE, cheats, tcp_link);
        } else {
            Self::step_n_frames(backend, UNCAPPED_BATCH_SIZE, cheats);
        }

        publish_framebuffer(shared_fb, backend.framebuffer());

        let result = FrameResult {
            rumble: backend.rumble_active(),
            audio_samples: Vec::new(),
            ui_data: ui::UiFrameData::default(),
            is_mbc7: backend.is_mbc7(),
            is_pocket_camera: backend.is_pocket_camera(),
            rewind_fill: rewind_buffer.fill_ratio(),
            apu_snapshot: None,
        };

        Self::send_frame(frame_tx, drain_rx, result);

        std::thread::yield_now();
    }
}
