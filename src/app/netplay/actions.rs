use super::*;
use crate::debug::MenuAction;

impl App {
    pub(in crate::app) fn handle_netplay_action(&mut self, action: &MenuAction) {
        let result = match action {
            MenuAction::ToggleNetplayWindow => {
                self.debug_windows
                    .netplay
                    .toggle_for_system(self.active_system);
                self.focus_state_dirty = true;
                Ok(())
            }
            MenuAction::HostNesNetplay => self.begin_netplay(None),
            MenuAction::JoinNesNetplay(invitation) => self.begin_netplay(Some(invitation.clone())),
            MenuAction::StopNesNetplay => {
                self.request_netplay_stop();
                Ok(())
            }
            MenuAction::SetNesNetplayPaused(paused) => {
                self.set_netplay_paused(*paused);
                Ok(())
            }
            MenuAction::SendNesNetplayChat(text) => {
                if self.netplay.running() {
                    if let Err(error) =
                        self.send_emu_command_checked(EmuCommand::SendNetplayChat(text.clone()))
                    {
                        self.debug_windows.netplay.chat_failed(error.to_string());
                    }
                } else {
                    self.debug_windows
                        .netplay
                        .chat_failed("Connect before sending a message.".into());
                }
                Ok(())
            }
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::HostTcpLink => self
                .host_tcp_link(Some(self.debug_windows.netplay.link_address.clone()))
                .map_err(anyhow::Error::msg),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::JoinTcpLink => self
                .join_tcp_link(Some(self.debug_windows.netplay.link_address.clone()))
                .map_err(anyhow::Error::msg),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::DisconnectLink => self.disconnect_link().map_err(anyhow::Error::msg),
            _ => return,
        };
        if let Err(error) = result {
            if self.debug_windows.netplay.linked_devices {
                self.debug_windows.netplay.link_status = error.to_string();
            }
            self.debug_windows.netplay.status = error.to_string();
            self.toast_manager.error(error.to_string());
        }
    }
}
