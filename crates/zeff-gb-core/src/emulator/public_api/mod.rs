mod audio;
mod debug;
mod queries;

use super::Emulator;

impl Emulator {
    pub fn preview_game_boy_link_peer(
        &self,
        peer: &Self,
    ) -> crate::hardware::bus::GameBoyLinkExchangePreview {
        self.bus.preview_game_boy_link_peer(&peer.bus)
    }

    pub fn sync_game_boy_link_peer(&mut self, peer: &mut Self) {
        self.bus.sync_game_boy_link_peer(&mut peer.bus);
    }

    pub fn try_sync_game_boy_link_peer(
        &mut self,
        peer: &mut Self,
    ) -> Result<
        crate::hardware::bus::GameBoyLinkExchangeOutcome,
        crate::hardware::bus::GameBoyLinkExchangeError,
    > {
        self.bus.try_sync_game_boy_link_peer(&mut peer.bus)
    }

    pub fn try_prepare_game_boy_link_peer(
        &mut self,
        peer: &mut Self,
    ) -> Result<
        crate::hardware::bus::GameBoyLinkPreparedExchange,
        crate::hardware::bus::GameBoyLinkExchangeError,
    > {
        self.bus.try_prepare_game_boy_link_peer(&mut peer.bus)
    }

    pub fn try_apply_prepared_game_boy_link_reply(
        &mut self,
        transfer: crate::hardware::bus::GameBoyLinkPreparedTransfer,
    ) -> Result<
        crate::hardware::bus::GameBoyLinkTransferExchange,
        crate::hardware::bus::GameBoyLinkExchangeError,
    > {
        self.bus.try_apply_prepared_game_boy_link_reply(transfer)
    }

    pub fn set_game_boy_link_peer_present(&mut self, present: bool) {
        self.bus.set_game_boy_link_peer_present(present);
    }

    pub fn restore_game_boy_link_peer_present_without_action(&mut self, present: bool) {
        self.bus
            .restore_game_boy_link_peer_present_without_action(present);
    }

    pub fn game_boy_link_state(&self) -> crate::hardware::bus::GameBoyLinkState {
        self.bus.game_boy_link_state()
    }

    pub fn game_boy_link_pending_master_response(&self) -> bool {
        self.bus.game_boy_link_pending_master_response()
    }

    pub fn game_boy_link_waiting_at_completion_boundary(&self) -> bool {
        self.bus.game_boy_link_waiting_at_completion_boundary()
    }

    pub fn take_game_boy_link_action(&mut self) -> Option<crate::hardware::bus::GameBoyLinkAction> {
        self.bus.take_game_boy_link_action()
    }

    pub fn game_boy_link_replay_state(&self) -> zeff_emu_common::replay::ReplayGameBoyLinkState {
        self.bus.game_boy_link_replay_state()
    }

    pub fn restore_game_boy_link_replay_state(
        &mut self,
        state: zeff_emu_common::replay::ReplayGameBoyLinkState,
    ) {
        self.bus.restore_game_boy_link_replay_state(state);
    }

    pub fn game_boy_link_reply_to_master_start(&self) -> crate::hardware::bus::GameBoyLinkReply {
        self.bus.game_boy_link_reply_to_master_start()
    }

    pub fn apply_game_boy_link_reply(
        &mut self,
        reply: crate::hardware::bus::GameBoyLinkReply,
    ) -> bool {
        self.bus.apply_game_boy_link_reply(reply)
    }

    pub fn schedule_game_boy_external_link_transfer(&mut self, peer_byte: u8, period: u64) -> bool {
        self.bus
            .schedule_game_boy_external_link_transfer(peer_byte, period)
    }

    pub fn complete_game_boy_external_link_transfer(&mut self, peer_byte: u8) -> bool {
        self.bus.complete_game_boy_external_link_transfer(peer_byte)
    }

    pub fn sync_game_boy_remote_link_peer(
        &mut self,
        peer_state: crate::hardware::bus::GameBoyLinkState,
    ) -> bool {
        self.bus.sync_game_boy_remote_link_peer(peer_state)
    }

    pub fn sync_game_boy_remote_link_peer_with_idle_response(
        &mut self,
        peer_state: crate::hardware::bus::GameBoyLinkState,
        idle_master_response: Option<u8>,
    ) -> bool {
        self.bus
            .sync_game_boy_remote_link_peer_with_idle_response(peer_state, idle_master_response)
    }

    pub fn set_input(&mut self, buttons: u8, dpad: u8) {
        if self.bus.apply_joypad_pressed_masks(buttons, dpad) {
            self.bus.if_reg |= 0x10;
        }
    }

    pub fn write_byte(&mut self, addr: u16, value: u8) {
        self.bus.write_byte(addr, value);
    }

    pub fn ppu_bg_palette_ram_snapshot(&self) -> [u8; 0x40] {
        self.bus.ppu_bg_palette_ram_snapshot()
    }

    pub fn ppu_obj_palette_ram_snapshot(&self) -> [u8; 0x40] {
        self.bus.ppu_obj_palette_ram_snapshot()
    }

    pub fn set_ppu_debug_flags(&mut self, bg: bool, window: bool, sprites: bool) {
        self.bus.set_ppu_debug_flags(bg, window, sprites);
    }

    pub fn set_dmg_palette_preset(&mut self, preset: crate::hardware::ppu::DmgPalettePreset) {
        self.bus.set_ppu_dmg_palette_preset(preset);
    }

    pub fn set_sgb_border_enabled(&mut self, enabled: bool) {
        self.bus.set_ppu_sgb_border_enabled(enabled);
    }

    pub fn sgb_border_active(&self) -> bool {
        self.bus.ppu_sgb_border_active()
    }

    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        self.bus.ppu_framebuffer_dimensions()
    }

    pub fn clear_rom_patches(&mut self) {
        self.bus.game_genie_patches.clear();
    }

    pub fn add_rom_patch(&mut self, patch: crate::cheats::CheatPatch) {
        self.bus.game_genie_patches.push(patch);
    }

    pub fn rom_patches(&self) -> &[crate::cheats::CheatPatch] {
        &self.bus.game_genie_patches
    }

    pub fn wram_snapshot(&self) -> &[u8] {
        &self.bus.wram
    }

    pub fn system_ram(&self) -> &[u8] {
        self.wram_snapshot()
    }

    pub fn vram_snapshot(&self) -> &[u8] {
        &self.bus.vram
    }

    pub fn video_ram_snapshot(&self) -> &[u8] {
        self.vram_snapshot()
    }
}
