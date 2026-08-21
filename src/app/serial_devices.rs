use super::App;
use crate::emu_thread::{EmuCommand, EmuResponse};
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read;

impl App {
    pub(super) fn consume_serial_device_response(
        &mut self,
        response: EmuResponse,
    ) -> Option<EmuResponse> {
        match response {
            EmuResponse::BardigunBarcodeScanStarted(byte_count) => {
                self.toast_manager
                    .success(format!("Bardigun scan started ({byte_count} bytes)"));
                None
            }
            EmuResponse::BardigunBarcodeScanFailed(error) => {
                self.toast_manager
                    .error(format!("Could not scan Bardigun card: {error}"));
                None
            }
            EmuResponse::BarcodeBoyScanStarted => {
                self.debug_windows.barcode_boy_digits.clear();
                self.toast_manager.success("Barcode Boy scan started");
                None
            }
            EmuResponse::BarcodeBoyScanFailed(error) => {
                self.debug_windows.barcode_boy_scan_open = true;
                self.toast_manager
                    .error(format!("Could not scan Barcode Boy card: {error}"));
                None
            }
            response => Some(response),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn scan_bardigun_barcode_file_dialog(&mut self) {
        if self.game_boy_serial_device
            != zeff_gb_core::hardware::GameBoySerialDevice::BardigunBarcodeReader
        {
            self.toast_manager
                .error("Attach the Bardigun Barcode Reader first");
            return;
        }

        let was_paused = self.pause_for_dialog();
        let path = crate::platform::FileDialog::new()
            .set_title("Scan Bardigun barcode data")
            .add_filter("Bardigun card data", &["btb", "bin", "raw", "dat"])
            .pick_file();
        self.resume_after_dialog(was_paused);

        let Some(path) = path else {
            return;
        };

        let result = (|| -> anyhow::Result<Vec<u8>> {
            let file = std::fs::File::open(&path)?;
            let mut bytes = Vec::new();
            file.take((zeff_gb_core::hardware::MAX_BARDIGUN_SCAN_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.is_empty() {
                anyhow::bail!("card data is empty");
            }
            if bytes.len() > zeff_gb_core::hardware::MAX_BARDIGUN_SCAN_BYTES {
                anyhow::bail!(
                    "card data exceeds the {}-byte limit",
                    zeff_gb_core::hardware::MAX_BARDIGUN_SCAN_BYTES
                );
            }
            Ok(bytes)
        })();

        match result {
            Ok(bytes) => {
                if let Some(thread) = &self.emu_thread {
                    thread.send(EmuCommand::QueueBardigunBarcodeScan(bytes));
                }
            }
            Err(err) => self
                .toast_manager
                .error(format!("Could not read Bardigun card data: {err}")),
        }
    }

    pub(super) fn trigger_barcode_boy_scan(&mut self, digits: String) {
        if self.game_boy_serial_device != zeff_gb_core::hardware::GameBoySerialDevice::BarcodeBoy {
            self.toast_manager.error("Attach Barcode Boy first");
            return;
        }
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::TriggerBarcodeBoyScan(digits));
        }
    }
}
