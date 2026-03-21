use anyhow::Result;
use std::path::PathBuf;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId,
};

use crate::{
    debug::{DebugViewerData, DebugWindowState, FpsTracker},
    emulator::Emulator,
    graphics,
    graphics::Graphics,
};

pub(crate) fn run(emulator: Option<Emulator>) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = App {
        emulator,
        gfx: None,
        window_id: None,
        fps_tracker: FpsTracker::new(),
        debug_windows: DebugWindowState::default(),
        exit_requested: false,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    emulator: Option<Emulator>,
    gfx: Option<Graphics>,
    window_id: Option<WindowId>,
    fps_tracker: FpsTracker,
    debug_windows: DebugWindowState,
    exit_requested: bool,
}

impl App {
    fn load_rom(&mut self, path: &std::path::Path) {
        match Emulator::from_rom(path) {
            Ok(emu) => {
                log::info!("Loaded ROM: {}", path.display());
                self.emulator = Some(emu);
                self.fps_tracker = FpsTracker::new();
            }
            Err(e) => {
                log::error!("Failed to load ROM '{}': {}", path.display(), e);
            }
        }
    }

    fn open_file_dialog(&mut self) {
        let file = rfd::FileDialog::new()
            .add_filter("Game Boy ROMs", &["gb", "gbc"])
            .add_filter("All files", &["*"])
            .set_title("Open ROM")
            .pick_file();

        if let Some(path) = file {
            self.load_rom(&path);
        }
    }

    fn handle_dropped_file(&mut self, path: PathBuf) {
        self.load_rom(&path);
    }

    fn tick(&mut self) {
        if let Some(emu) = &mut self.emulator {
            emu.step_frame();
        }

        let fb_copy = self.emulator.as_ref().map(|emu| emu.framebuffer().to_vec());

        self.fps_tracker.tick();

        let debug_info = self.emulator.as_ref().map(|emu| {
            let mut info = emu.snapshot();
            info.fps = self.fps_tracker.fps();
            info
        });

        let viewer_data = self.emulator.as_ref().map(|emu| DebugViewerData {
            vram: emu.vram().to_vec(),
            oam: emu.oam().to_vec(),
            ppu: emu.ppu_registers(),
        });

        let Some(gfx) = self.gfx.as_mut() else { return; };

        if let Some(fb) = &fb_copy {
            gfx.upload_framebuffer(fb);
        }

        match gfx.render(debug_info.as_ref(), viewer_data.as_ref(), &mut self.debug_windows) {
            Ok(result) => {
                if result.open_file_requested {
                    self.open_file_dialog();
                }
            }
            Err(graphics::FrameError::Outdated) | Err(graphics::FrameError::Lost) => {
                let size = gfx.size();
                gfx.resize(size.width, size.height);
            }
            Err(graphics::FrameError::Timeout)
            | Err(graphics::FrameError::Occluded)
            | Err(graphics::FrameError::Validation) => {}
            Err(graphics::FrameError::OutOfMemory) => self.exit_requested = true,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }

        let gfx = pollster::block_on(Graphics::new(event_loop))
            .expect("failed to initialize graphics");
        self.window_id = Some(gfx.window().id());
        self.gfx = Some(gfx);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.gfx.is_none() {
            return;
        }
        if Some(window_id) != self.window_id {
            return;
        }

        {
            let gfx = self.gfx.as_mut().expect("graphics initialized");
            if gfx.handle_event(&event) {
                return;
            }
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                let gfx = self.gfx.as_mut().expect("graphics initialized");
                gfx.resize(size.width, size.height)
            }
            WindowEvent::DroppedFile(path) => self.handle_dropped_file(path),
            WindowEvent::RedrawRequested => self.tick(),

            _ => {}
        }

        if self.exit_requested {
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(gfx) = &self.gfx {
            gfx.window().request_redraw();
        }
    }
}