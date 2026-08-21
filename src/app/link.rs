use super::App;

#[cfg(not(target_arch = "wasm32"))]
use crate::emu_thread::{EmuCommand, EmuResponse, TcpLinkMode};

impl App {
    #[cfg(not(target_arch = "wasm32"))]
    fn detach_game_boy_serial_device_for_link(&mut self) {
        if self.active_system != crate::emu_backend::ActiveSystem::GameBoy
            || self.game_boy_serial_device
                == zeff_gb_core::hardware::GameBoySerialDevice::Disconnected
        {
            return;
        }

        self.game_boy_serial_device = zeff_gb_core::hardware::GameBoySerialDevice::Disconnected;
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::SetGameBoySerialDevice(
                zeff_gb_core::hardware::GameBoySerialDevice::Disconnected,
            ));
        }
    }

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
            EmuResponse::LinkConnected {
                label,
                frame_count,
                game_boy_cpu_cycles,
                game_boy_link_state,
            } => {
                self.tcp_link_active = true;
                self.resume_if_paused_by_unfocus_for_link();
                self.record_game_boy_link_state_replay_event(
                    *frame_count,
                    *game_boy_cpu_cycles,
                    *game_boy_link_state,
                );
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
            EmuResponse::LinkDisconnected {
                frame_count,
                game_boy_cpu_cycles,
                game_boy_link_state,
            } => {
                let was_active = self.tcp_link_active;
                self.tcp_link_active = false;
                self.pause_for_unfocus_if_needed_after_link_end();
                self.record_game_boy_link_state_replay_event(
                    *frame_count,
                    *game_boy_cpu_cycles,
                    *game_boy_link_state,
                );
                if was_active {
                    self.toast_manager.info("Link disconnected");
                }
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
    pub(super) fn host_tcp_link(&mut self) -> bool {
        if self.emu_thread.is_none() {
            self.toast_manager
                .error("Load a GB/GBC or WonderSwan/WSC ROM before hosting link");
            return false;
        }
        if crate::link::remote_link_system_for_active_system(self.active_system).is_none() {
            self.toast_manager
                .error("TCP link currently supports GB/GBC and WonderSwan/WSC only");
            return false;
        }
        if self.recording.is_replay_active() {
            self.toast_manager
                .error("Stop replay activity before starting a TCP link");
            return false;
        }

        let bind_addr = self.tcp_link_addr();
        self.detach_game_boy_serial_device_for_link();
        self.tcp_link_active = true;
        self.resume_if_paused_by_unfocus_for_link();
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::StartTcpLink(TcpLinkMode::Host { bind_addr }));
        }
        true
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn join_tcp_link(&mut self) -> bool {
        if self.emu_thread.is_none() {
            self.toast_manager
                .error("Load a GB/GBC or WonderSwan/WSC ROM before joining link");
            return false;
        }
        if crate::link::remote_link_system_for_active_system(self.active_system).is_none() {
            self.toast_manager
                .error("TCP link currently supports GB/GBC and WonderSwan/WSC only");
            return false;
        }
        if self.recording.is_replay_active() {
            self.toast_manager
                .error("Stop replay activity before starting a TCP link");
            return false;
        }

        let connect_addr = self.tcp_link_addr();
        self.detach_game_boy_serial_device_for_link();
        self.tcp_link_active = true;
        self.resume_if_paused_by_unfocus_for_link();
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::StartTcpLink(TcpLinkMode::Join { connect_addr }));
        }
        true
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
        activity_keeps_running_while_unfocused(
            self.tcp_link_active,
            self.recording.is_replay_active(),
            self.live_control.is_enabled(),
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn link_keeps_running(&self) -> bool {
        false
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn resume_if_paused_by_unfocus_for_link(&mut self) {
        if self.paused_by_unfocus {
            self.paused_by_unfocus = false;
            self.speed.paused = false;
            self.timing.last_frame_time = crate::platform::Instant::now();
            self.toast_manager.set_paused(false);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn pause_for_unfocus_if_needed_after_link_end(&mut self) {
        if !self.window_focused
            && self.settings.emulation.pause_on_unfocus
            && !self.link_keeps_running()
            && !self.speed.paused
        {
            self.paused_by_unfocus = true;
            self.speed.paused = true;
            self.toast_manager.set_paused(true);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn record_game_boy_link_state_replay_event(
        &mut self,
        frame_count: u64,
        game_boy_cpu_cycles: Option<u64>,
        state: Option<zeff_emu_common::replay::ReplayGameBoyLinkState>,
    ) {
        if self.active_system != crate::emu_backend::ActiveSystem::GameBoy {
            return;
        }
        let Some(state) = state else {
            return;
        };
        let origin = self.recording.replay_recording_origin;
        let Some(frame) = frame_count.checked_sub(origin.frame) else {
            log::warn!(
                "Dropping GB replay link-state event before replay origin: frame={} origin={}",
                frame_count,
                origin.frame
            );
            return;
        };
        let Some(recorder) = self.recording.replay_recorder.as_mut() else {
            return;
        };
        if let (Some(tick), Some(base_tick)) = (game_boy_cpu_cycles, origin.game_boy_tick) {
            let Some(tick) = tick.checked_sub(base_tick) else {
                log::warn!(
                    "Dropping GB replay link-state event before replay origin tick: tick={} origin={}",
                    tick,
                    base_tick
                );
                return;
            };
            recorder.record_event(
                zeff_emu_common::replay::ReplayEvent::GameBoyLinkStateAtTick { frame, tick, state },
            );
        } else {
            recorder.record_event(zeff_emu_common::replay::ReplayEvent::GameBoyLinkState {
                frame,
                state,
            });
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn activity_keeps_running_while_unfocused(
    tcp_link_active: bool,
    replay_active: bool,
    live_control_active: bool,
) -> bool {
    tcp_link_active || replay_active || live_control_active
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::activity_keeps_running_while_unfocused;

    #[test]
    fn replay_activity_keeps_unfocused_native_instance_running() {
        assert!(activity_keeps_running_while_unfocused(false, true, false));
        assert!(activity_keeps_running_while_unfocused(true, false, false));
        assert!(activity_keeps_running_while_unfocused(false, false, true));
        assert!(!activity_keeps_running_while_unfocused(false, false, false));
    }
}
