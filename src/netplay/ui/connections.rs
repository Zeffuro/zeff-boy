use super::*;

impl Ui {
    #[cfg(target_arch = "wasm32")]
    pub(super) fn draw_link(&mut self, _: &mut egui::Ui, _: ActiveSystem) -> Option<MenuAction> {
        None
    }

    pub(crate) fn open_for_system(&mut self, system: ActiveSystem) {
        if !self.active && !self.link_active {
            self.linked_devices = super::super::capabilities::linked_devices(system)
                && !super::super::capabilities::rollback_session(system);
        }
        self.open();
    }

    pub(crate) fn toggle_for_system(&mut self, system: ActiveSystem) {
        if self.is_open() {
            self.close();
        } else {
            self.open_for_system(system);
        }
    }

    pub(crate) fn lobby_options(&self) -> Result<super::super::connect::lobby::Options> {
        let options = super::super::connect::lobby::Options {
            url: self.lobby_url.trim().to_owned(),
            access_token: self.lobby_key.clone(),
            input_delay: InputDelay::new(self.input_delay)?,
        };
        options.validate()?;
        Ok(options)
    }

    pub(super) fn draw_lobby(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Server");
            ui.add(egui::TextEdit::singleline(&mut self.lobby_url).desired_width(f32::INFINITY));
        });
        ui.horizontal(|ui| {
            ui.label("Key");
            ui.add(
                egui::TextEdit::singleline(&mut self.lobby_key)
                    .password(true)
                    .hint_text("Private server only")
                    .desired_width(f32::INFINITY),
            );
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn draw_link(
        &mut self,
        ui: &mut egui::Ui,
        system: ActiveSystem,
    ) -> Option<MenuAction> {
        let supported = super::super::capabilities::linked_devices(system);
        if !supported {
            ui.label("Link cable supports Game Boy and WonderSwan.");
            return None;
        }
        ui.label(if self.link_status.is_empty() {
            "One device per player"
        } else {
            &self.link_status
        });
        ui.horizontal(|ui| {
            ui.label("Address");
            ui.add_enabled(
                !self.link_active,
                egui::TextEdit::singleline(&mut self.link_address).desired_width(f32::INFINITY),
            );
        });
        if self.link_active {
            return ui
                .button("Disconnect")
                .clicked()
                .then_some(MenuAction::DisconnectLink);
        }
        ui.small("TCP link. Use a trusted local network.");
        ui.horizontal(|ui| {
            if ui.button("Host").clicked() {
                Some(MenuAction::HostTcpLink)
            } else if ui.button("Join").clicked() {
                Some(MenuAction::JoinTcpLink)
            } else {
                None
            }
        })
        .inner
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn menu_chooses_loaded_adapter_and_keeps_active_session_mode() {
        let mut ui = Ui::for_route(false);
        for spec in crate::emu_backend::system_specs() {
            ui.open_for_system(spec.system);
            assert_eq!(
                ui.linked_devices,
                super::super::super::capabilities::linked_devices(spec.system)
                    && !super::super::super::capabilities::rollback_session(spec.system)
            );
        }
        ui.linked_devices = true;
        ui.link_active = true;
        ui.open_for_system(ActiveSystem::Nes);
        assert!(ui.linked_devices);
        ui.link_active = false;
        ui.active = true;
        ui.linked_devices = false;
        ui.open_for_system(ActiveSystem::GameBoy);
        assert!(!ui.linked_devices);
    }

    #[test]
    fn lobby_key_is_local_and_options_accept_public_empty_key() {
        let mut ui = Ui::for_route(false);
        ui.lobby = true;
        let options = ui.lobby_options().unwrap();
        assert!(options.access_token.is_empty());
        ui.lobby_key = "private-key".repeat(4);
        let value = super::super::super::connect::lobby::invitation(
            &ui.lobby_url,
            &"a".repeat(24),
            [7; 32],
            InputDelay::default(),
        );
        assert!(!value.contains(&ui.lobby_key));
        assert_eq!(ui.invitation_delay(&value).unwrap(), InputDelay::default());
        ui.lobby_url = "wss://another.example/v1/ws".into();
        assert!(ui.invitation_delay(&value).is_err());
    }
}
