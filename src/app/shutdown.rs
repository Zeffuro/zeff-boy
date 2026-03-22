use super::App;

impl App {
    pub(super) fn stop_emu_thread(&mut self) {
        if let Some(mut thread) = self.emu_thread.take() {
            thread.shutdown();
        }
    }

    fn flush_battery_sram_on_shutdown(&self) {
        let Some(emu) = self.emulator.as_ref() else {
            return;
        };

        let emu = emu.lock().expect("emulator mutex poisoned");
        match emu.flush_battery_sram() {
            Ok(Some(saved)) => log::info!("Saved battery RAM to {}", saved),
            Ok(None) => {}
            Err(err) => log::error!("Failed to save battery RAM on exit: {}", err),
        }
    }

    pub(super) fn perform_shutdown(&mut self) {
        if self.shutdown_performed {
            return;
        }
        self.shutdown_performed = true;

        self.stop_emu_thread();
        self.flush_battery_sram_on_shutdown();

        self.gfx = None;
        self.audio = None;
        self.window_id = None;
        self.latest_frame = None;
    }
}
