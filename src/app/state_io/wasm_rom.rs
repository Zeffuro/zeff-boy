use super::App;

#[cfg(target_arch = "wasm32")]
use {
    crate::debug::FpsTracker,
    crate::emu_backend::ActiveSystem,
    crate::emu_thread::{EmuCommand, EmuThread},
    crate::platform::Instant,
    std::path::PathBuf,
};

impl App {
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn check_pending_rom(&mut self) {
        let data = self.pending_rom_load.borrow_mut().take();
        if let Some((name, bytes)) = data {
            self.load_rom_from_bytes(name, bytes);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn load_rom_from_bytes(&mut self, name: String, data: Vec<u8>) {
        self.stop_emu_thread();
        self.stop_camera_capture();

        self.frames_in_flight = 0;
        self.cached_ui_data = None;
        self.recycled.clear();
        self.debug_windows.last_disasm_pc = None;

        let path = PathBuf::from(&name);
        let is_zip = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));

        let (rom_path, rom_data, system) = if is_zip {
            match super::extract_rom_from_zip_bytes(&data, &name) {
                Ok((vp, d)) => match ActiveSystem::from_path(&vp) {
                    Some(s) => (vp, d, s),
                    None => {
                        self.toast_manager
                            .error("Unsupported ROM type inside ZIP".to_string());
                        return;
                    }
                },
                Err(e) => {
                    let msg = format!("{e:#}");
                    log::warn!("{msg}");
                    self.toast_manager.error(msg);
                    return;
                }
            }
        } else {
            match ActiveSystem::from_path(&path) {
                Some(s) => (path.clone(), data, s),
                None => {
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("(none)");
                    self.toast_manager
                        .error(format!("Unsupported file type: .{ext}"));
                    return;
                }
            }
        };

        let (backend, _original_crc) =
            match self.init_backend(system, &path, &rom_path, Some(rom_data)) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("Failed to load ROM '{}': {}", name, e);
                    self.toast_manager.error(format!("Failed to load ROM: {e}"));
                    return;
                }
            };

        let rom_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("ROM")
            .to_string();
        log::info!("Loaded ROM: {}", name);

        self.rom_info.is_mbc7 = backend.is_mbc7();
        self.rom_info.is_pocket_camera = backend.is_pocket_camera();
        self.rom_info.rom_path = Some(rom_path);
        self.rom_info.rom_hash = Some(backend.rom_hash());
        self.active_system = system;

        let (native_w, native_h) = system.screen_size();
        if let Some(gfx) = self.gfx.as_mut() {
            gfx.set_native_size(native_w, native_h);
        }

        self.emu_thread = Some(EmuThread::spawn(backend));
        self.fps_tracker = FpsTracker::new();
        self.timing.last_frame_time = Instant::now();

        if self.timing.uncapped_speed
            && let Some(thread) = &self.emu_thread
        {
            thread.send(EmuCommand::SetUncapped(true));
        }

        self.toast_manager.info(format!("Loaded {rom_name}"));
        self.refresh_slot_info();
    }
}
