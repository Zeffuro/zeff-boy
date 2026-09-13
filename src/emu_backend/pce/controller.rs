use super::*;

impl PceBackend {
    pub(super) fn set_pad_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        if let Some(mouse) = self.machine.controller_input_mut().mouse_mut() {
            let pad = map_pad_buttons(buttons_pressed, dpad_pressed);
            mouse.set_buttons(self.mouse_host_buttons | pad);
            let horizontal = i16::from(pad.contains(PadButtons::LEFT))
                - i16::from(pad.contains(PadButtons::RIGHT));
            let vertical =
                i16::from(pad.contains(PadButtons::UP)) - i16::from(pad.contains(PadButtons::DOWN));
            mouse.accumulate_motion(horizontal * 4, vertical * 4);
            return;
        }
        if self.set_multitap_pad_input(
            zeff_pce_core::hardware::MultitapPort::One,
            buttons_pressed,
            dpad_pressed,
        ) {
            return;
        }
        let controller = self.machine.controller_input_mut();
        if let Some(pad) = controller.six_button_pad_mut() {
            pad.standard_pad_mut()
                .set_buttons(map_pad_buttons(buttons_pressed, dpad_pressed));
            pad.set_extra_buttons(map_six_button_extra_buttons(buttons_pressed));
        } else if let Some(pad) = controller.two_button_pad_mut() {
            pad.set_buttons(map_pad_buttons(buttons_pressed, dpad_pressed));
        }
    }

    pub(super) fn set_multitap_pad_input(
        &mut self,
        port: zeff_pce_core::hardware::MultitapPort,
        buttons_pressed: u8,
        dpad_pressed: u8,
    ) -> bool {
        let Some(multitap) = self.machine.controller_input_mut().multitap_mut() else {
            return false;
        };
        match multitap.port_mut(port) {
            zeff_pce_core::hardware::MultitapDevice::TwoButton(pad) => {
                pad.set_buttons(map_pad_buttons(buttons_pressed, dpad_pressed));
            }
            zeff_pce_core::hardware::MultitapDevice::SixButton(pad) => {
                pad.standard_pad_mut()
                    .set_buttons(map_pad_buttons(buttons_pressed, dpad_pressed));
                pad.set_extra_buttons(map_six_button_extra_buttons(buttons_pressed));
            }
            zeff_pce_core::hardware::MultitapDevice::Disconnected => {}
        }
        true
    }

    pub(super) fn effective_controller_mode(
        &self,
        requested: PceControllerMode,
    ) -> PceControllerMode {
        match requested {
            PceControllerMode::Automatic => {
                automatic_controller_mode(self.controller_profile_hash())
            }
            explicit => explicit,
        }
    }

    pub(crate) fn update_controller_mode(&mut self, requested: PceControllerMode) {
        let effective = self.effective_controller_mode(requested);
        if self.pce_controller_mode == effective {
            return;
        }
        let device = match effective {
            PceControllerMode::Mouse => zeff_pce_core::hardware::ControllerDevice::Mouse(
                zeff_pce_core::hardware::PceMouse::new(),
            ),
            PceControllerMode::SixButton => zeff_pce_core::hardware::ControllerDevice::SixButton(
                zeff_pce_core::hardware::SixButtonPad::new(),
            ),
            PceControllerMode::Multitap => {
                zeff_pce_core::hardware::ControllerDevice::Multitap(FivePortMultitap::new([
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                ]))
            }
            PceControllerMode::Automatic | PceControllerMode::TwoButton => {
                zeff_pce_core::hardware::ControllerDevice::TwoButton(
                    zeff_pce_core::hardware::TwoButtonPad::new(),
                )
            }
        };
        self.machine.devices_mut().set_controller_device(device);
        self.pce_controller_mode = effective;
    }

    pub(crate) fn update_memory_base_mode(&mut self, requested: PceMemoryBaseMode) {
        let enabled = match requested {
            PceMemoryBaseMode::Automatic => automatic_memory_base_enabled(self.source_disc_hash()),
            PceMemoryBaseMode::Enabled => true,
            PceMemoryBaseMode::Disabled => false,
        };
        self.machine
            .devices_mut()
            .controller_mut()
            .set_memory_base128_connected(enabled);
        self.pce_memory_base_mode = if enabled {
            PceMemoryBaseMode::Enabled
        } else {
            PceMemoryBaseMode::Disabled
        };
    }
}
