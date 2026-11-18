use super::App;

impl App {
    pub(in crate::app) fn record_current_tas_input_and_advance(&mut self) -> anyhow::Result<()> {
        if !self.tas_control.can_record_live_input() {
            anyhow::bail!("no completed TAS execution is awaiting a live input frame");
        }
        let (profile, _) = self.tas_control.linked_identity().ok_or_else(|| {
            anyhow::anyhow!("no completed TAS execution is awaiting a live input frame")
        })?;
        let mut input = crate::tas_project::TasInputFrame::default();
        match profile {
            crate::emu_thread::TasExecutionProfile::DirectMdThreeButton
            | crate::emu_thread::TasExecutionProfile::DirectMdSixButton => {
                let (buttons, dpad) = self.current_host_joypad_input();
                input.players[0].buttons = buttons;
                input.players[0].dpad = dpad;
                let (buttons, dpad) = self.current_host_joypad_p2_input();
                input.players[1].buttons = buttons;
                input.players[1].dpad = dpad;
            }
            crate::emu_thread::TasExecutionProfile::DirectColecoCartridge => {
                input.coleco = self.host_input.coleco_tas_controllers()?;
            }
            crate::emu_thread::TasExecutionProfile::DirectSmsCartridge
            | crate::emu_thread::TasExecutionProfile::DirectSg1000Cartridge => {
                let (p1_buttons, p1_dpad) = self.current_host_joypad_input();
                let (p2_buttons, p2_dpad) = self.current_host_joypad_p2_input();
                input.players[0].buttons = p1_buttons;
                input.players[0].dpad = p1_dpad;
                input.players[1].buttons = p2_buttons;
                input.players[1].dpad = p2_dpad;
            }
            crate::emu_thread::TasExecutionProfile::DirectPceMultitapHuCard
            | crate::emu_thread::TasExecutionProfile::DirectPceMultitapCd => {
                let players = [
                    self.current_host_joypad_input(),
                    self.current_host_joypad_p2_input(),
                    self.current_host_joypad_p3_input(),
                    self.current_host_joypad_p4_input(),
                    self.current_host_joypad_p5_input(),
                ];
                for (player, (buttons, dpad)) in input.players.iter_mut().zip(players) {
                    player.buttons = buttons;
                    player.dpad = dpad;
                }
            }
            crate::emu_thread::TasExecutionProfile::DirectWsCartridge => {
                let (p1_buttons, p1_dpad) = self.current_host_joypad_input();
                input.players[0].buttons = p1_buttons;
                input.players[0].dpad = p1_dpad;
            }
            crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge => {
                let (p1_buttons, p1_dpad) = self.current_host_joypad_input();
                input.players[0].buttons = p1_buttons;
                input.players[0].dpad = p1_dpad;
            }
            crate::emu_thread::TasExecutionProfile::DirectGbaCartridge => {
                let (p1_buttons, p1_dpad) = self.current_host_joypad_input();
                input.players[0].buttons = p1_buttons;
                input.players[0].dpad = p1_dpad;
            }
            profile => {
                let (p1_buttons, p1_dpad) = self.current_host_joypad_input();
                input.players[0].buttons = p1_buttons;
                input.players[0].dpad = p1_dpad;
                if matches!(
                    profile,
                    crate::emu_thread::TasExecutionProfile::DirectNesCartridge
                        | crate::emu_thread::TasExecutionProfile::DirectFdsDisk
                ) {
                    let (p2_buttons, p2_dpad) = self.current_host_joypad_p2_input();
                    input.players[1].buttons = p2_buttons;
                    input.players[1].dpad = p2_dpad;
                    if profile == crate::emu_thread::TasExecutionProfile::DirectNesCartridge {
                        let zapper = self.nes_zapper_input();
                        input.zapper.enabled = zapper.enabled;
                        input.zapper.trigger = zapper.trigger;
                        input.zapper.hit = zapper.hit;
                        input.zapper.screen_pos = zapper.screen_pos.map(|(x, y)| [x, y]);
                    }
                }
            }
        }
        let autofire = self.preview_autofire_tas_input(&mut input);
        if crate::emu_backend::loader::md_product::md_pad_profile(profile).is_some() {
            input = crate::emu_backend::loader::md_product::canonicalize_md_tas_host_input(
                profile, input,
            )?;
        }
        self.record_live_tas_frame(input)?;
        self.pending_tas_autofire = self
            .tas_control
            .live_frame_in_flight()
            .then_some(autofire)
            .flatten();
        Ok(())
    }
}
