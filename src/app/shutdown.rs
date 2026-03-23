use super::App;

impl App {
    pub(super) fn stop_emu_thread(&mut self) {
        if let Some(mut thread) = self.emu_thread.take() {
            thread.shutdown(); // Sends Shutdown → flushes SRAM → joins thread
        }
    }

    pub(super) fn perform_shutdown(&mut self) {
        if self.shutdown_performed {
            return;
        }
        self.shutdown_performed = true;

        // Emu thread shutdown flushes SRAM automatically
        self.stop_emu_thread();

        self.gfx = None;
        self.audio = None;
        self.window_id = None;
        self.latest_frame = None;
    }
}
