use std::path::PathBuf;

use crossbeam_channel::Sender;

use crate::emu_backend::EmuBackend;

use super::{EmuResponse, EmuThread};

impl EmuThread {
    pub(crate) fn respond_load_state(
        backend: &mut EmuBackend,
        result: anyhow::Result<()>,
        path_label: String,
        buttons_pressed: u8,
        dpad_pressed: u8,
    ) -> EmuResponse {
        match result {
            Ok(()) => {
                backend.set_input(buttons_pressed, dpad_pressed);
                let fb = backend.framebuffer().to_vec();
                EmuResponse::LoadStateOk {
                    path: path_label,
                    framebuffer: fb,
                }
            }
            Err(err) => EmuResponse::LoadStateFailed(err.to_string()),
        }
    }

    pub(crate) fn save_state_async(
        backend: &EmuBackend,
        path: PathBuf,
        resp_tx: &Sender<EmuResponse>,
        send_resp: &impl Fn(EmuResponse) -> bool,
    ) -> bool {
        match Self::encode_current_state(backend) {
            Ok(bytes) => {
                let tx = resp_tx.clone();
                std::thread::spawn(move || {
                    let resp = match crate::save_paths::write_state_bytes_to_file(&path, &bytes) {
                        Ok(()) => EmuResponse::SaveStateOk(path.display().to_string()),
                        Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
                    };
                    if tx.send(resp).is_err() {
                        log::warn!("save-state response dropped (receiver closed)");
                    }
                });
                true
            }
            Err(err) => send_resp(EmuResponse::SaveStateFailed(err.to_string())),
        }
    }

    pub(crate) fn handle_rewind(
        backend: &mut EmuBackend,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
    ) -> EmuResponse {
        let current_state = Self::encode_current_state(backend).ok();
        while let Some(rewind_frame) = rewind_buffer.pop() {
            if let Some(current) = current_state.as_ref()
                && rewind_frame.state_bytes == *current
                && !rewind_buffer.is_empty()
            {
                continue;
            }
            match backend.load_state_from_bytes(rewind_frame.state_bytes) {
                Ok(()) => {
                    let fb = if rewind_frame.framebuffer.is_empty() {
                        backend.framebuffer().to_vec()
                    } else {
                        rewind_frame.framebuffer
                    };
                    return EmuResponse::RewindOk { framebuffer: fb };
                }
                Err(err) => {
                    log::warn!("Rewind restore failed: {}", err);
                    return EmuResponse::RewindFailed("rewind restore failed".to_string());
                }
            }
        }

        EmuResponse::RewindFailed("no rewind data".to_string())
    }

    pub(crate) fn encode_current_state(backend: &EmuBackend) -> anyhow::Result<Vec<u8>> {
        backend.encode_state_bytes()
    }

    pub(crate) fn capture_rewind_snapshot(
        backend: &EmuBackend,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        enabled: bool,
    ) {
        if enabled
            && rewind_buffer.tick()
            && let Ok(bytes) = Self::encode_current_state(backend)
        {
            rewind_buffer.push(&bytes, backend.framebuffer());
        }
    }
}
