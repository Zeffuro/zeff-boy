use super::App;
use crate::emu_thread::{EmuCommand, EmuResponse, TasControlCommandKind};
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read;

fn commit_serial_device_selection(
    current: &mut zeff_gb_core::hardware::GameBoySerialDevice,
    selected: zeff_gb_core::hardware::GameBoySerialDevice,
    sent: bool,
) {
    if sent {
        *current = selected;
    }
}

impl App {
    fn serial_peripheral_change_allowed(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        let link_inactive = !self.tcp_link_active;
        #[cfg(target_arch = "wasm32")]
        let link_inactive = true;
        self.emu_thread.is_some() && !self.recording.is_replay_active() && link_inactive
    }

    pub(super) fn set_game_boy_serial_device(
        &mut self,
        device: zeff_gb_core::hardware::GameBoySerialDevice,
    ) {
        if let Err(error) =
            self.preflight_emu_command_kind(TasControlCommandKind::MediaOrPeripheral)
        {
            self.toast_manager.error(error.to_string());
            return;
        }
        if !self.serial_peripheral_change_allowed() {
            self.toast_manager.error(
                "Disconnect the link and stop replay activity before changing the serial device",
            );
            return;
        }
        if let Err(error) =
            self.send_emu_command_checked(EmuCommand::SetGameBoySerialDevice(device))
        {
            self.toast_manager.error(error.to_string());
            return;
        }
        commit_serial_device_selection(&mut self.game_boy_serial_device, device, true);
    }

    pub(super) fn open_barcode_boy_scan(&mut self) {
        if let Err(error) =
            self.preflight_emu_command_kind(TasControlCommandKind::MediaOrPeripheral)
        {
            self.toast_manager.error(error.to_string());
            return;
        }
        if !self.serial_peripheral_change_allowed() {
            self.toast_manager
                .error("Disconnect the link and stop replay activity before scanning a card");
            return;
        }
        self.debug_windows.barcode_boy_scan_open = true;
    }

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
        if let Err(error) =
            self.preflight_emu_command_kind(TasControlCommandKind::MediaOrPeripheral)
        {
            self.toast_manager.error(error.to_string());
            return;
        }
        if !self.serial_peripheral_change_allowed() {
            self.toast_manager
                .error("Disconnect the link and stop replay activity before scanning a card");
            return;
        }
        if self.game_boy_serial_device
            != zeff_gb_core::hardware::GameBoySerialDevice::BardigunBarcodeReader
        {
            self.toast_manager
                .error("Attach the Bardigun Barcode Reader first");
            return;
        }

        self.pause_for_dialog();
        let path = crate::platform::FileDialog::new()
            .set_title("Scan Bardigun barcode data")
            .add_filter("Bardigun card data", &["btb", "bin", "raw", "dat"])
            .pick_file();
        self.resume_after_dialog();

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
                if let Err(error) =
                    self.send_emu_command_checked(EmuCommand::QueueBardigunBarcodeScan(bytes))
                {
                    self.toast_manager.error(error.to_string());
                }
            }
            Err(err) => self
                .toast_manager
                .error(format!("Could not read Bardigun card data: {err}")),
        }
    }

    pub(super) fn trigger_barcode_boy_scan(&mut self, digits: String) {
        if let Err(error) =
            self.preflight_emu_command_kind(TasControlCommandKind::MediaOrPeripheral)
        {
            self.toast_manager.error(error.to_string());
            return;
        }
        if !self.serial_peripheral_change_allowed() {
            self.toast_manager
                .error("Disconnect the link and stop replay activity before scanning a card");
            return;
        }
        if self.game_boy_serial_device != zeff_gb_core::hardware::GameBoySerialDevice::BarcodeBoy {
            self.toast_manager.error("Attach Barcode Boy first");
            return;
        }
        if let Err(error) = self.send_emu_command_checked(EmuCommand::TriggerBarcodeBoyScan(digits))
        {
            self.toast_manager.error(error.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_selection_changes_only_after_send() {
        use zeff_gb_core::hardware::GameBoySerialDevice;

        let mut current = GameBoySerialDevice::Disconnected;
        commit_serial_device_selection(&mut current, GameBoySerialDevice::BarcodeBoy, false);
        assert_eq!(current, GameBoySerialDevice::Disconnected);
        commit_serial_device_selection(&mut current, GameBoySerialDevice::BarcodeBoy, true);
        assert_eq!(current, GameBoySerialDevice::BarcodeBoy);
    }
}
