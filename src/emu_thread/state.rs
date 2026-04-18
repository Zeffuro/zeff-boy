use std::path::PathBuf;

use crossbeam_channel::Sender;

use crate::emu_backend::EmuBackend;

use super::{EmuResponse, EmuThread};

impl EmuThread {
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
}
