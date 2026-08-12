use super::App;

#[cfg(not(target_arch = "wasm32"))]
use crate::emu_thread::{EmuCommand, EmuResponse, TcpLinkMode};

impl App {
    #[cfg(not(target_arch = "wasm32"))]
    fn tcp_link_addr(&self) -> String {
        let addr = self.settings.emulation.tcp_link_addr.trim();
        if addr.is_empty() {
            crate::link::transport::native::DEFAULT_TCP_LINK_ADDR.to_string()
        } else {
            addr.to_string()
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn handle_link_response(&mut self, response: &EmuResponse) -> bool {
        match response {
            EmuResponse::LinkPending(label) => {
                self.tcp_link_active = true;
                self.resume_if_paused_by_unfocus_for_link();
                self.toast_manager.info(format!("Link {label}"));
                true
            }
            EmuResponse::LinkConnected(label) => {
                self.tcp_link_active = true;
                self.resume_if_paused_by_unfocus_for_link();
                self.toast_manager
                    .success(format!("Link connected ({label})"));
                true
            }
            EmuResponse::LinkFailed(message) => {
                self.tcp_link_active = false;
                self.pause_for_unfocus_if_needed_after_link_end();
                self.toast_manager.error(format!("Link failed: {message}"));
                true
            }
            EmuResponse::LinkDisconnected => {
                self.tcp_link_active = false;
                self.pause_for_unfocus_if_needed_after_link_end();
                self.toast_manager.info("Link disconnected");
                true
            }
            _ => false,
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn handle_link_response(
        &mut self,
        _response: &crate::emu_thread::EmuResponse,
    ) -> bool {
        false
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn host_tcp_link(&mut self) {
        if self.emu_thread.is_none() {
            self.toast_manager
                .error("Load a GB/GBC ROM before hosting link");
            return;
        }
        if self.active_system != crate::emu_backend::ActiveSystem::GameBoy {
            self.toast_manager
                .error("TCP link currently supports GB/GBC only");
            return;
        }

        let bind_addr = self.tcp_link_addr();
        self.tcp_link_active = true;
        self.resume_if_paused_by_unfocus_for_link();
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::StartTcpLink(TcpLinkMode::Host { bind_addr }));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn join_tcp_link(&mut self) {
        if self.emu_thread.is_none() {
            self.toast_manager
                .error("Load a GB/GBC ROM before joining link");
            return;
        }
        if self.active_system != crate::emu_backend::ActiveSystem::GameBoy {
            self.toast_manager
                .error("TCP link currently supports GB/GBC only");
            return;
        }

        let connect_addr = self.tcp_link_addr();
        self.tcp_link_active = true;
        self.resume_if_paused_by_unfocus_for_link();
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::StartTcpLink(TcpLinkMode::Join { connect_addr }));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn disconnect_link(&mut self) {
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::DisconnectLink);
        }
        self.tcp_link_active = false;
        self.pause_for_unfocus_if_needed_after_link_end();
        self.toast_manager.info("Link disconnected");
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn link_keeps_running(&self) -> bool {
        self.tcp_link_active
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn link_keeps_running(&self) -> bool {
        false
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn resume_if_paused_by_unfocus_for_link(&mut self) {
        if self.paused_by_unfocus {
            self.paused_by_unfocus = false;
            self.speed.paused = false;
            self.toast_manager.set_paused(false);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn pause_for_unfocus_if_needed_after_link_end(&mut self) {
        if !self.window_focused && self.settings.emulation.pause_on_unfocus && !self.speed.paused {
            self.paused_by_unfocus = true;
            self.speed.paused = true;
            self.toast_manager.set_paused(true);
        }
    }
}
