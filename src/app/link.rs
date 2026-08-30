use super::App;

#[cfg(not(target_arch = "wasm32"))]
use crate::emu_thread::{EmuCommand, EmuResponse, TasControlCommandKind, TcpLinkMode};

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
enum TcpLinkStart {
    Host,
    Join,
}

#[cfg(not(target_arch = "wasm32"))]
fn commit_tcp_link_start(
    active: &mut bool,
    configured_addr: &mut String,
    resolved_addr: String,
    sent: bool,
) {
    if sent {
        *configured_addr = resolved_addr;
        *active = true;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn resolved_tcp_link_addr(configured: &str, requested: Option<&str>) -> String {
    let addr = requested.unwrap_or(configured).trim();
    if addr.is_empty() {
        crate::link::transport::native::DEFAULT_TCP_LINK_ADDR.to_string()
    } else {
        addr.to_string()
    }
}

impl App {
    #[cfg(not(target_arch = "wasm32"))]
    fn detach_game_boy_serial_device_for_link(&mut self) -> Result<(), String> {
        if self.active_system != crate::emu_backend::ActiveSystem::GameBoy
            || self.game_boy_serial_device
                == zeff_gb_core::hardware::GameBoySerialDevice::Disconnected
        {
            return Ok(());
        }

        let disconnected = zeff_gb_core::hardware::GameBoySerialDevice::Disconnected;
        self.send_emu_command_checked(EmuCommand::SetGameBoySerialDevice(disconnected))
            .map_err(|error| error.to_string())?;
        self.game_boy_serial_device = disconnected;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn tcp_link_addr(&self, requested: Option<&str>) -> String {
        resolved_tcp_link_addr(&self.settings.emulation.tcp_link_addr, requested)
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
    pub(super) fn host_tcp_link(&mut self, requested_addr: Option<String>) -> Result<(), String> {
        self.start_tcp_link(TcpLinkStart::Host, requested_addr)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn join_tcp_link(&mut self, requested_addr: Option<String>) -> Result<(), String> {
        self.start_tcp_link(TcpLinkStart::Join, requested_addr)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_tcp_link(
        &mut self,
        start: TcpLinkStart,
        requested_addr: Option<String>,
    ) -> Result<(), String> {
        self.preflight_emu_command_kind(TasControlCommandKind::Link)
            .map_err(|error| error.to_string())?;
        if crate::link::remote_link_system_for_active_system(self.active_system).is_none() {
            return Err("TCP link currently supports GB/GBC and WonderSwan/WSC only".to_string());
        }
        if self.recording.is_replay_active() {
            return Err("Stop replay activity before starting a TCP link".to_string());
        }

        let addr = self.tcp_link_addr(requested_addr.as_deref());
        self.detach_game_boy_serial_device_for_link()?;
        let mode = match start {
            TcpLinkStart::Host => TcpLinkMode::Host {
                bind_addr: addr.clone(),
            },
            TcpLinkStart::Join => TcpLinkMode::Join {
                connect_addr: addr.clone(),
            },
        };
        self.send_emu_command_checked(EmuCommand::StartTcpLink(mode))
            .map_err(|error| error.to_string())?;
        commit_tcp_link_start(
            &mut self.tcp_link_active,
            &mut self.settings.emulation.tcp_link_addr,
            addr,
            true,
        );
        self.resume_if_paused_by_unfocus_for_link();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn disconnect_link(&mut self) -> Result<(), String> {
        self.preflight_emu_command_kind(TasControlCommandKind::Link)
            .map_err(|error| error.to_string())?;
        self.send_emu_command_checked(EmuCommand::DisconnectLink)
            .map_err(|error| error.to_string())?;
        self.tcp_link_active = false;
        self.pause_for_unfocus_if_needed_after_link_end();
        self.toast_manager.info("Link disconnected");
        Ok(())
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
        self.pause_state.set_focus(false);
        self.recompute_pause();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn pause_for_unfocus_if_needed_after_link_end(&mut self) {
        if !self.window_focused
            && self.settings.emulation.pause_on_unfocus
            && !self.link_keeps_running()
        {
            self.pause_state.set_focus(true);
            self.recompute_pause();
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
    use super::{
        activity_keeps_running_while_unfocused, commit_tcp_link_start, resolved_tcp_link_addr,
    };

    #[test]
    fn replay_activity_keeps_unfocused_native_instance_running() {
        assert!(activity_keeps_running_while_unfocused(false, true, false));
        assert!(activity_keeps_running_while_unfocused(true, false, false));
        assert!(activity_keeps_running_while_unfocused(false, false, true));
        assert!(!activity_keeps_running_while_unfocused(false, false, false));
    }

    #[test]
    fn link_state_and_setting_change_only_after_send() {
        let mut active = false;
        let mut addr = "old".to_string();
        commit_tcp_link_start(&mut active, &mut addr, "new".to_string(), false);
        assert!(!active);
        assert_eq!(addr, "old");

        commit_tcp_link_start(&mut active, &mut addr, "new".to_string(), true);
        assert!(active);
        assert_eq!(addr, "new");
    }

    #[test]
    fn link_address_resolution_trims_and_defaults_before_commit() {
        assert_eq!(
            resolved_tcp_link_addr("old", Some(" 127.0.0.1:9000 ")),
            "127.0.0.1:9000"
        );
        assert_eq!(
            resolved_tcp_link_addr("old", Some("  ")),
            crate::link::transport::native::DEFAULT_TCP_LINK_ADDR
        );
        assert_eq!(resolved_tcp_link_addr(" saved ", None), "saved");
    }
}
