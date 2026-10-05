use std::net::{IpAddr, SocketAddr};

use anyhow::{Context, Result, ensure};
use zeff_netplay::endpoint::ConnectionScope;
use zeff_netplay::rollback::InputDelay;

use crate::debug::MenuAction;
use crate::emu_backend::ActiveSystem;
use crate::platform::Instant;

use super::connect::HostOptions;

mod chat;
mod connections;
#[cfg(not(target_arch = "wasm32"))]
mod host_addresses;

pub(crate) struct Ui {
    pub(crate) active: bool,
    pub(crate) connected: bool,
    pub(crate) local_pause: bool,
    pub(crate) status: String,
    pub(crate) metrics: String,
    pub(crate) invitation: String,
    pub(crate) private_network: bool,
    pub(crate) lobby: bool,
    pub(crate) lobby_url: String,
    pub(crate) lobby_key: String,
    pub(crate) linked_devices: bool,
    pub(crate) link_active: bool,
    pub(crate) link_status: String,
    pub(crate) link_address: String,
    pub(crate) host_address: String,
    pub(crate) host_port: u16,
    pub(crate) allow_different_versions: bool,
    pub(crate) input_delay: u64,
    pub(crate) chat: super::chat::Chat,
    chat_draft: String,
    chat_pending: bool,
    open: bool,
    focus_requested: bool,
    host_window_focused: bool,
    last_host_render: Instant,
    joining: bool,
    join: String,
    #[cfg(not(target_arch = "wasm32"))]
    host_addresses: host_addresses::HostAddresses,
}

impl Default for Ui {
    fn default() -> Self {
        let forwarded = std::env::var("ZEFF_NETPLAY_TEST_FORWARD").as_deref() == Ok("1");
        Self::for_route(forwarded)
    }
}

impl Ui {
    fn for_route(forwarded: bool) -> Self {
        Self {
            active: false,
            connected: false,
            local_pause: false,
            status: String::new(),
            metrics: String::new(),
            invitation: String::new(),
            private_network: forwarded,
            lobby: cfg!(target_arch = "wasm32"),
            lobby_url: super::connect::lobby::DEFAULT_URL.into(),
            lobby_key: String::new(),
            linked_devices: false,
            link_active: false,
            link_status: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            link_address: crate::link::transport::native::DEFAULT_TCP_LINK_ADDR.into(),
            #[cfg(target_arch = "wasm32")]
            link_address: String::new(),
            host_address: if forwarded {
                "127.0.0.1".into()
            } else {
                String::new()
            },
            host_port: 8766,
            allow_different_versions: false,
            input_delay: if forwarded {
                0
            } else {
                InputDelay::default().frames()
            },
            chat: super::chat::Chat::default(),
            chat_draft: String::new(),
            chat_pending: false,
            open: false,
            focus_requested: false,
            host_window_focused: false,
            last_host_render: Instant::now(),
            joining: false,
            join: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            host_addresses: host_addresses::HostAddresses::new(forwarded),
        }
    }
}

impl Ui {
    pub(crate) fn scope(&self) -> ConnectionScope {
        if self.private_network {
            ConnectionScope::TrustedPrivate
        } else {
            ConnectionScope::Loopback
        }
    }

    pub(crate) fn host_options(&self) -> Result<HostOptions> {
        let address = if self.private_network {
            #[cfg(not(target_arch = "wasm32"))]
            self.host_addresses.validate(&self.host_address)?;
            ensure!(self.host_port != 0, "netplay host port must be nonzero");
            let ip: IpAddr = self
                .host_address
                .trim()
                .parse()
                .context("enter the host computer's numeric private IP address")?;
            SocketAddr::new(ip, self.host_port)
        } else {
            SocketAddr::from(([127, 0, 0, 1], 0))
        };
        let options = HostOptions {
            address,
            scope: self.scope(),
            input_delay: InputDelay::new(self.input_delay)?,
        };
        options.validate()?;
        Ok(options)
    }

    pub(crate) fn invitation_delay(&self, invitation: &str) -> Result<InputDelay> {
        if self.lobby {
            super::connect::lobby::parse_invitation(invitation, self.lobby_url.trim())
                .map(|(_, _, delay)| delay)
        } else {
            super::connect::invitation_delay(invitation, self.scope())
        }
    }

    pub(crate) fn open(&mut self) {
        self.open = true;
        self.focus_requested = true;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.focus_requested = false;
        self.host_window_focused = false;
    }

    pub(crate) fn take_focus_request(&mut self) -> bool {
        std::mem::take(&mut self.focus_requested)
    }

    pub(crate) fn host_window_focused(&self) -> bool {
        self.host_window_focused
    }

    pub(crate) fn set_host_window_focused(&mut self, focused: bool) {
        self.host_window_focused = focused;
    }

    pub(crate) fn last_host_render(&self) -> Instant {
        self.last_host_render
    }

    pub(crate) fn mark_host_rendered(&mut self) {
        self.last_host_render = Instant::now();
    }

    pub(crate) fn draw_contents(
        &mut self,
        ui: &mut egui::Ui,
        system: ActiveSystem,
    ) -> Option<MenuAction> {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.draw_controls(ui, system))
            .inner
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn draw_browser_window(
        &mut self,
        ctx: &egui::Context,
        system: ActiveSystem,
    ) -> Option<MenuAction> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut action = None;
        let min_height = (ctx.viewport_rect().height() - 64.0).clamp(80.0, 240.0);
        egui::Window::new("Netplay")
            .open(&mut open)
            .default_size([360.0, 300.0])
            .min_height(min_height)
            .max_width(420.0)
            .show(ctx, |ui| action = self.draw_contents(ui, system));
        if !open {
            self.close();
        }
        action
    }

    fn draw_controls(&mut self, ui: &mut egui::Ui, system: ActiveSystem) -> Option<MenuAction> {
        if !cfg!(target_arch = "wasm32") && !self.active && !self.link_active {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.linked_devices, false, "Shared console");
                ui.selectable_value(&mut self.linked_devices, true, "Link cable");
            });
            ui.separator();
        }
        if self.linked_devices {
            return self.draw_link(ui, system);
        }
        let idle = format!("{} · two players", system.code().to_uppercase());
        ui.label(if !self.status.is_empty() {
            &self.status
        } else if self.connected {
            "Connected"
        } else if self.active {
            "Connecting"
        } else {
            &idle
        });
        if self.active {
            ui.label(format!("Delay: {} frames", self.input_delay));
            if !self.invitation.is_empty() && ui.button("Copy invitation").clicked() {
                ui.ctx().copy_text(self.invitation.clone());
            }
            let action = ui
                .horizontal_wrapped(|ui| {
                    if self.connected
                        && ui
                            .button(if self.local_pause { "Resume" } else { "Pause" })
                            .clicked()
                    {
                        return Some(MenuAction::SetNesNetplayPaused(!self.local_pause));
                    }
                    ui.button(if self.connected {
                        "Disconnect"
                    } else {
                        "Cancel"
                    })
                    .on_hover_text("End the session. The game resets and pauses.")
                    .clicked()
                    .then_some(MenuAction::StopNesNetplay)
                })
                .inner;
            if action.is_some() {
                return action;
            }
            if (self.connected || self.chat.messages().next().is_some())
                && let Some(action) = self.draw_chat(ui)
            {
                return Some(action);
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.joining, false, "Host");
                ui.selectable_value(&mut self.joining, true, "Join");
                ui.separator();
                if !cfg!(target_arch = "wasm32")
                    && ui
                        .selectable_label(!self.lobby && !self.private_network, "Same PC")
                        .clicked()
                {
                    self.lobby = false;
                    self.private_network = false;
                }
                if !cfg!(target_arch = "wasm32")
                    && ui
                        .selectable_label(!self.lobby && self.private_network, "LAN")
                        .clicked()
                {
                    self.lobby = false;
                    self.private_network = true;
                }
                #[cfg(not(target_arch = "wasm32"))]
                ui.selectable_value(&mut self.lobby, true, "Lobby");
            });
            if self.lobby {
                self.draw_lobby(ui);
            }
            if self.joining {
                ui.add(
                    egui::TextEdit::singleline(&mut self.join)
                        .desired_width(f32::INFINITY)
                        .hint_text("Paste invitation"),
                );
                if let Ok(input_delay) = self.invitation_delay(self.join.trim()) {
                    ui.label(format!("Host delay: {} frames", input_delay.frames()));
                }
            } else if self.private_network && !self.lobby {
                #[cfg(not(target_arch = "wasm32"))]
                self.host_addresses.draw(ui, &mut self.host_address);
            }
            if !self.joining {
                ui.add(
                    egui::Slider::new(&mut self.input_delay, InputDelay::MIN..=InputDelay::MAX)
                        .text("Delay (frames)"),
                ).on_hover_text("0 adds no netplay input delay. Raise it if a slower connection stutters. Fixed for this session.");
            }
            ui.small("New game. Session saves are discarded.");
            if ui
                .add_enabled(
                    super::capabilities::shared_console(system)
                        && if self.joining {
                            !self.join.trim().is_empty()
                        } else {
                            if self.lobby {
                                self.lobby_options().is_ok()
                            } else {
                                self.host_options().is_ok()
                            }
                        },
                    egui::Button::new(if self.joining {
                        "Connect"
                    } else {
                        "Start hosting"
                    }),
                )
                .on_hover_text("Use the same game and compatible builds on both devices.")
                .clicked()
            {
                return Some(if self.joining {
                    MenuAction::JoinNesNetplay(self.join.trim().to_owned())
                } else {
                    MenuAction::HostNesNetplay
                });
            }
            if !super::capabilities::shared_console(system) {
                ui.label("No shared-console adapter for this system.");
            }
        }
        if !self.active {
            self.draw_options(ui, system);
            if self.chat.messages().next().is_some()
                && let Some(action) = self.draw_chat(ui)
            {
                return Some(action);
            }
        }
        if self.active {
            ui.small("Player 1 controls on both devices.")
                .on_hover_text("Click the game to play. Unfocused input is neutral.");
            self.draw_stats(ui, system);
        }
        None
    }

    fn draw_options(&mut self, ui: &mut egui::Ui, system: ActiveSystem) {
        let compatible = system == ActiveSystem::Nes && super::compatibility::available();
        if !compatible && (self.joining || !self.private_network || self.lobby) {
            return;
        }
        ui.collapsing("Options", |ui| {
            if !self.joining && self.private_network && !self.lobby {
                ui.horizontal(|ui| {
                    ui.label("Port");
                    ui.add(egui::DragValue::new(&mut self.host_port).range(1..=u16::MAX));
                });
            }
            if compatible {
                ui.checkbox(
                    &mut self.allow_different_versions,
                    "Allow compatible different versions",
                )
                .on_hover_text("Both players must opt in for this session.");
            }
        });
    }

    fn draw_stats(&mut self, ui: &mut egui::Ui, system: ActiveSystem) {
        let compatible_build = system == ActiveSystem::Nes && super::compatibility::available();
        ui.collapsing("Stats", |ui| {
            if !self.metrics.is_empty() {
                ui.small(&self.metrics);
            }
            let signed_pair =
                system == ActiveSystem::Nes && super::compatibility::signed_pair_available();
            ui.label(if signed_pair {
                "Builds: paired"
            } else if compatible_build {
                "Builds: matching or qualified"
            } else {
                "Builds: matching"
            });
            if system == ActiveSystem::Nes
                && let Err(error) = super::compatibility::certificate_status()
            {
                ui.small("Pair certificate unavailable")
                    .on_hover_text(error.to_string());
            }
            if self.lobby {
                ui.small("Direct · encrypted")
                    .on_hover_text("The lobby handles setup. Gameplay travels between players.");
            } else if self.private_network {
                ui.small("LAN · authenticated TCP")
                    .on_hover_text("Use a trusted network. TCP is not encrypted.");
            }
        });
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod window_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn default_host_stays_ephemeral_loopback() {
        let ui = Ui::for_route(false);
        assert_eq!(ui.scope(), ConnectionScope::Loopback);
        assert_eq!(ui.host_port, 8766);
        let options = ui.host_options().unwrap();
        assert_eq!(options.scope, ConnectionScope::Loopback);
        assert_eq!(
            options.address,
            "127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap()
        );
        assert_eq!(options.input_delay, InputDelay::default());
    }

    #[test]
    fn forwarded_test_route_starts_at_zero_delay_on_fixed_loopback_port() {
        let ui = Ui::for_route(true);
        let options = ui.host_options().unwrap();
        assert_eq!(options.scope, ConnectionScope::TrustedPrivate);
        assert_eq!(
            options.address,
            "127.0.0.1:8766".parse::<std::net::SocketAddr>().unwrap()
        );
        assert_eq!(options.input_delay.frames(), 0);
    }

    #[test]
    fn private_host_requires_explicit_address_and_nonzero_port() {
        let mut ui = Ui {
            private_network: true,
            ..Ui::default()
        };
        assert_eq!(ui.scope(), ConnectionScope::TrustedPrivate);
        assert!(ui.host_options().is_err());
        for ip in ["192.168.1.10", "100.64.1.2", "fd00::1"] {
            ui.host_address = ip.to_owned();
            let options = ui.host_options().unwrap();
            assert_eq!(options.scope, ConnectionScope::TrustedPrivate);
            assert_eq!(options.address, SocketAddr::new(ip.parse().unwrap(), 8766));
        }
        ui.host_port = 23456;
        assert_eq!(ui.host_options().unwrap().address.port(), 23456);
        ui.host_port = 0;
        assert!(ui.host_options().is_err());
    }

    #[test]
    fn private_host_rejects_public_wildcard_and_nonnumeric_addresses() {
        let mut ui = Ui {
            private_network: true,
            ..Ui::default()
        };
        for ip in [
            "8.8.8.8",
            "0.0.0.0",
            "::",
            "host.local",
            "192.168.1.10:8766",
            "fe80::1%3",
        ] {
            ui.host_address = ip.to_owned();
            assert!(ui.host_options().is_err(), "{ip}");
        }
    }

    #[test]
    fn returning_to_same_computer_ignores_private_host_settings() {
        let mut ui = Ui {
            private_network: true,
            host_address: "8.8.8.8".to_owned(),
            host_port: 0,
            ..Ui::default()
        };
        assert!(ui.host_options().is_err());
        ui.private_network = false;
        assert_eq!(ui.scope(), ConnectionScope::Loopback);
        assert_eq!(
            ui.host_options().unwrap().address,
            "127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap()
        );
    }

    #[test]
    fn host_delay_is_validated_and_join_uses_the_invitation_delay() {
        let mut ui = Ui::default();
        for frames in InputDelay::MIN..=InputDelay::MAX {
            ui.input_delay = frames;
            assert_eq!(ui.host_options().unwrap().input_delay.frames(), frames);
        }
        for frames in [9, u64::MAX] {
            ui.input_delay = frames;
            assert!(ui.host_options().is_err());
        }
        ui.input_delay = 8;
        let invitation = format!("127.0.0.1:8766/{}", "ab".repeat(32));
        assert_eq!(ui.invitation_delay(&invitation).unwrap().frames(), 2);
        assert_eq!(
            ui.invitation_delay(&format!("{invitation}/4"))
                .unwrap()
                .frames(),
            4
        );
        assert_eq!(ui.input_delay, 8);
    }
}
