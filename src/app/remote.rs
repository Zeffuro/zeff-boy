use serde_json::{Value, json};

use super::App;
use super::command_gate::EmuCommandSendError;
#[cfg(test)]
use crate::emu_backend::CoreCapabilities;
use crate::emu_thread::EmuCommand;
#[cfg(test)]
use crate::input::HostButton;
use crate::live_control::{LiveCommand, LiveInput, LiveReply, PendingButtonRelease};
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, MemoryRegionView};
use zeff_emu_common::save_ram::SaveRamKind;
#[cfg(test)]
use zeff_emu_common::system::CoreFamily;

mod artifacts;
mod core_features;
mod graphics;
mod json_helpers;
mod memory;
#[cfg(not(target_arch = "wasm32"))]
mod tas;

use core_features::core_features_json;
use json_helpers::{cpu_debug_json, live_speed_mode_name, live_system_name};

fn replay_stop_live_reply(
    outcome: Result<(), EmuCommandSendError>,
    status: impl FnOnce() -> Value,
) -> LiveReply {
    match outcome {
        Ok(()) => LiveReply::ok(status()),
        Err(error) => LiveReply::error(error.to_string()),
    }
}

impl App {
    pub(super) fn drain_live_control(&mut self) {
        while let Some(request) = self.live_control.try_recv() {
            request.respond_with(|command| self.handle_live_command(command));
        }
    }

    pub(super) fn limit_frames_for_live_button_releases(&self, frames: usize) -> usize {
        pending_release_frame_limit(&self.live_button_releases, frames)
    }

    pub(super) fn advance_live_button_releases(&mut self, frames: usize) {
        for (player, input) in
            advance_pending_button_releases(&mut self.live_button_releases, frames)
        {
            set_remote_input(&mut self.host_input, player, input, false);
        }
    }

    fn handle_live_command(&mut self, command: LiveCommand) -> LiveReply {
        match command {
            LiveCommand::Status => LiveReply::ok(self.live_status_json()),
            LiveCommand::DebugInfo => {
                self.remote_debug_frames_remaining = 3;
                LiveReply::ok(self.live_debug_json())
            }
            LiveCommand::Pause => {
                self.set_user_paused(true);
                LiveReply::ok(self.live_status_json())
            }
            LiveCommand::Resume => {
                self.set_user_paused(false);
                LiveReply::ok(self.live_status_json())
            }
            LiveCommand::TogglePause => {
                self.toggle_user_paused();
                LiveReply::ok(self.live_status_json())
            }
            LiveCommand::FrameAdvance => {
                if let Err(error) = self.preflight_emu_command_kind(
                    crate::emu_thread::TasControlCommandKind::FrameExecution,
                ) {
                    return LiveReply::error(error.to_string());
                }
                self.set_user_paused(true);
                self.debug_requests.frame_advance = true;
                self.remote_debug_frames_remaining = 3;
                LiveReply::ok(self.live_status_json())
            }
            LiveCommand::SetSlowMotion(enabled) => {
                self.settings.emulation.slow_motion_enabled = enabled;
                LiveReply::ok(self.live_status_json())
            }
            LiveCommand::SetFastForward(enabled) => {
                self.speed.fast_forward_held = enabled;
                LiveReply::ok(self.live_status_json())
            }
            LiveCommand::SetUncapped(enabled) => {
                if let Err(error) = self.send_emu_command_checked(EmuCommand::SetUncapped(
                    enabled && self.recording.allows_uncapped_worker(),
                )) {
                    return LiveReply::error(error.to_string());
                }
                self.timing.uncapped_speed = enabled;
                self.settings.emulation.uncapped_speed = enabled;
                LiveReply::ok(self.live_status_json())
            }
            LiveCommand::Button {
                player,
                key,
                pressed,
            } => {
                if player > 2 && self.active_system != crate::emu_backend::ActiveSystem::Pce {
                    return LiveReply::error(
                        "players 3 through 5 are only available for PC Engine",
                    );
                }
                let input = LiveInput::Button(key);
                set_remote_input(&mut self.host_input, player, input, pressed);
                if !pressed {
                    self.live_button_releases
                        .retain(|release| !same_pending_input(release, player, input));
                }
                LiveReply::ok(self.live_input_json())
            }
            LiveCommand::Tap {
                player,
                key,
                frames,
            } => {
                if player > 2 && self.active_system != crate::emu_backend::ActiveSystem::Pce {
                    return LiveReply::error(
                        "players 3 through 5 are only available for PC Engine",
                    );
                }
                let input = LiveInput::Button(key);
                set_remote_input(&mut self.host_input, player, input, true);
                self.live_button_releases
                    .retain(|release| !same_pending_input(release, player, input));
                self.live_button_releases.push(PendingButtonRelease {
                    player,
                    input,
                    frames_remaining: frames,
                });
                LiveReply::ok(self.live_input_json())
            }
            LiveCommand::ColecoKeypad {
                player,
                key,
                pressed,
            } => {
                if self.active_system != crate::emu_backend::ActiveSystem::Coleco {
                    return LiveReply::error(
                        "ColecoVision keypad input is only available for ColecoVision",
                    );
                }
                if player > 2 {
                    return LiveReply::error("ColecoVision supports players 1 and 2 only");
                }
                let input = LiveInput::ColecoKeypad(key);
                set_remote_input(&mut self.host_input, player, input, pressed);
                if !pressed {
                    self.live_button_releases
                        .retain(|release| !same_pending_input(release, player, input));
                }
                LiveReply::ok(self.live_input_json())
            }
            LiveCommand::TapColecoKeypad {
                player,
                key,
                frames,
            } => {
                if self.active_system != crate::emu_backend::ActiveSystem::Coleco {
                    return LiveReply::error(
                        "ColecoVision keypad input is only available for ColecoVision",
                    );
                }
                if player > 2 {
                    return LiveReply::error("ColecoVision supports players 1 and 2 only");
                }
                let input = LiveInput::ColecoKeypad(key);
                set_remote_input(&mut self.host_input, player, input, true);
                self.live_button_releases
                    .retain(|release| !same_pending_input(release, player, input));
                self.live_button_releases.push(PendingButtonRelease {
                    player,
                    input,
                    frames_remaining: frames,
                });
                LiveReply::ok(self.live_input_json())
            }
            LiveCommand::Zapper {
                enabled,
                trigger,
                hit,
                screen_pos,
            } => {
                self.remote_zapper = if enabled {
                    Some(crate::emu_thread::ZapperInput {
                        enabled,
                        trigger,
                        hit,
                        screen_pos,
                    })
                } else {
                    None
                };
                LiveReply::ok(self.live_input_json())
            }
            LiveCommand::Screenshot { path } => match self.write_live_screenshot(path) {
                Ok(result) => LiveReply::ok(result),
                Err(err) => LiveReply::error(err.to_string()),
            },
            LiveCommand::SaveState { path } => match self.live_save_state(path) {
                Ok(result) => LiveReply::ok(result),
                Err(err) => LiveReply::error(err.to_string()),
            },
            LiveCommand::LoadState { path } => match self.live_load_state(path) {
                Ok(result) => LiveReply::ok(result),
                Err(err) => LiveReply::error(err.to_string()),
            },
            LiveCommand::SaveStateSlot { slot } => {
                if self.core_supports_save_states() {
                    self.save_state_slot(slot);
                    LiveReply::ok(self.live_status_json())
                } else {
                    LiveReply::error("the active core does not support save states")
                }
            }
            LiveCommand::LoadStateSlot { slot } => {
                if self.core_supports_save_states() {
                    self.load_state_slot(slot);
                    LiveReply::ok(self.live_status_json())
                } else {
                    LiveReply::error("the active core does not support save states")
                }
            }
            LiveCommand::StartReplayRecording { path } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.start_replay_recording_to_path(path) {
                        Ok(()) => LiveReply::ok(self.live_status_json()),
                        Err(err) => LiveReply::error(err.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = path;
                    LiveReply::error("replay recording is not available on web")
                }
            }
            LiveCommand::StopReplayRecording => {
                let outcome = self.stop_replay_recording();
                replay_stop_live_reply(outcome, || self.live_status_json())
            }
            LiveCommand::HostLink { addr } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.host_tcp_link(addr) {
                        Ok(()) => LiveReply::ok(self.live_status_json()),
                        Err(error) => LiveReply::error(error),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = addr;
                    LiveReply::error("TCP link is not available on web")
                }
            }
            LiveCommand::JoinLink { addr } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.join_tcp_link(addr) {
                        Ok(()) => LiveReply::ok(self.live_status_json()),
                        Err(error) => LiveReply::error(error),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = addr;
                    LiveReply::error("TCP link is not available on web")
                }
            }
            LiveCommand::DisconnectLink => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.disconnect_link() {
                        Ok(()) => LiveReply::ok(self.live_status_json()),
                        Err(error) => LiveReply::error(error),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    LiveReply::error("TCP link is not available on web")
                }
            }
            LiveCommand::TasOpenProject { path } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_open_tas_project(path) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = path;
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasCreateProject {
                path,
                replace_existing,
            } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_create_tas_project(path, replace_existing) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = (path, replace_existing);
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasStatus => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    LiveReply::ok(self.live_tas_status_json())
                }
                #[cfg(target_arch = "wasm32")]
                {
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasSelectBoundary { boundary } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_select_tas_boundary(boundary) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = boundary;
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasSelectRange { start, end } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_select_tas_range(start, end) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = (start, end);
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasDeleteSelectedFrames => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_delete_selected_tas_frames() {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasInsertNeutralFrames { boundary, count } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_insert_neutral_tas_frames(boundary, count) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = (boundary, count);
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasSetDigitalInput {
                frame,
                player,
                input,
                pressed,
            } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_set_tas_digital_input(frame, player, input, pressed) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = (frame, player, input, pressed);
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasGoToSelection => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_go_to_tas_selection() {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasForkBranch { id, name } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_fork_tas_branch(id, name) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = (id, name);
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasSetRealtimeRecording { active } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_set_tas_realtime_recording(active) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = active;
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasSetPlayback { active } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_set_tas_playback(active) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = active;
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasLink { at_end, record } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_link_tas(at_end, record) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = (at_end, record);
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasReloadGame => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_reload_tas_game() {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasRecordFrame { mode } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_record_tas_frame(mode) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::TasDisconnect { keep } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match self.live_disconnect_tas(keep) {
                        Ok(result) => LiveReply::ok(result),
                        Err(error) => LiveReply::error(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = keep;
                    LiveReply::error("live TAS control is not available on web")
                }
            }
            LiveCommand::MemoryRead {
                space,
                start,
                length,
            } => LiveReply::ok(self.live_memory_json(&space, start, length)),
            LiveCommand::GraphicsInfo => LiveReply::ok(self.live_graphics_json()),
        }
    }

    #[cfg(test)]
    pub(in crate::app) fn handle_live_command_for_test(
        &mut self,
        command: LiveCommand,
    ) -> LiveReply {
        self.handle_live_command(command)
    }

    fn live_status_json(&self) -> Value {
        #[cfg(not(target_arch = "wasm32"))]
        let tcp_link_active = self.tcp_link_active;
        #[cfg(target_arch = "wasm32")]
        let tcp_link_active = false;
        let framebuffer_bytes = self
            .last_displayed_frame
            .as_ref()
            .or(self.latest_frame.as_ref())
            .map_or(0, |frame| frame.len());
        let (screen_width, screen_height) = self.active_display_size();
        json!({
            "enabled": self.live_control.is_enabled(),
            "addr": self.live_control.addr().map(|addr| addr.to_string()),
            "rom_loaded": self.emu_thread.is_some(),
            "system": live_system_name(self.active_system),
            "paused": self.speed.paused,
            "speed_mode": live_speed_mode_name(self.speed_mode()),
            "slow_motion": self.settings.emulation.slow_motion_enabled,
            "fast_forward": self.speed.fast_forward_held,
            "uncapped": self.timing.uncapped_speed,
            "tcp_link_active": tcp_link_active,
            "frames_in_flight": self.frames_in_flight,
            "window_focused": self.window_focused,
            "paused_by_unfocus": self.pause_state.focus_paused(),
            "unfocus_pause_suppressed": self.suppress_unfocus_pause_until_focus,
            "replay": {
                "starting": self.recording.is_replay_start_pending(),
                "recording": self.recording.replay_recorder.is_some(),
                "recorded_frames": self.recording
                    .replay_recorder
                    .as_ref()
                    .map(|recorder| recorder.frame_count())
                    .unwrap_or(0),
                "recording_base_frame": self.recording.replay_recording_origin.frame,
                "recording_base_game_boy_tick": self
                    .recording
                    .replay_recording_origin
                    .game_boy_tick,
                "playing": self.recording.replay_player.is_some(),
                "saving": self.recording.is_replay_finalizing(),
                "pending_batches": self.recording.pending_replay_batches.len(),
            },
            "active_save_slot": self.active_save_slot,
            "rewind_fill": self.rewind.fill,
            "framebuffer": {
                "bytes": framebuffer_bytes,
                "screen_width": screen_width,
                "screen_height": screen_height,
            },
            "debug_info_cached": self.cached_ui_data.as_ref().and_then(|data| data.cpu_debug.as_ref()).is_some(),
            "core_features": self.cached_ui_data.as_ref().and_then(|data| {
                data.core_features.as_ref().map(core_features_json)
            }),
        })
    }

    fn live_debug_json(&self) -> Value {
        let cpu_debug = self
            .cached_ui_data
            .as_ref()
            .and_then(|data| data.cpu_debug.as_ref())
            .map(cpu_debug_json);
        let has_cpu_debug = cpu_debug.is_some();

        json!({
            "status": self.live_status_json(),
            "cpu": cpu_debug,
            "note": if has_cpu_debug {
                Value::Null
            } else {
                json!("No cached CPU debug data yet. Run debug_info again after a frame, or use frame_advance while paused.")
            },
        })
    }

    fn live_input_json(&self) -> Value {
        let (buttons, dpad) = self.current_host_joypad_input();
        let (buttons_p2, dpad_p2) = self.current_host_joypad_p2_input();
        let (buttons_p3, dpad_p3) = self.current_host_joypad_p3_input();
        let (buttons_p4, dpad_p4) = self.current_host_joypad_p4_input();
        let (buttons_p5, dpad_p5) = self.current_host_joypad_p5_input();
        let mut players = serde_json::Map::new();
        players.insert("1".to_owned(), json!({ "buttons": buttons, "dpad": dpad }));
        players.insert(
            "2".to_owned(),
            json!({ "buttons": buttons_p2, "dpad": dpad_p2 }),
        );
        if self.active_system == crate::emu_backend::ActiveSystem::Pce {
            players.insert(
                "3".to_owned(),
                json!({ "buttons": buttons_p3, "dpad": dpad_p3 }),
            );
            players.insert(
                "4".to_owned(),
                json!({ "buttons": buttons_p4, "dpad": dpad_p4 }),
            );
            players.insert(
                "5".to_owned(),
                json!({ "buttons": buttons_p5, "dpad": dpad_p5 }),
            );
        }
        json!({
            "buttons": buttons,
            "dpad": dpad,
            "buttons_p2": buttons_p2,
            "dpad_p2": dpad_p2,
            "players": players,
            "coleco_keypad": (self.active_system == crate::emu_backend::ActiveSystem::Coleco).then(|| json!({
                "player1": self.host_input.coleco_keypad_pressed(1),
                "player2": self.host_input.coleco_keypad_pressed(2),
            })),
            "zapper": self.remote_zapper.map(|zapper| json!({
                "enabled": zapper.enabled,
                "trigger": zapper.trigger,
                "hit": zapper.hit,
                "screen_pos": zapper.screen_pos.map(|(x, y)| json!({ "x": x, "y": y })),
            })),
        })
    }
}

fn memory_region_json(region: &MemoryRegionDescriptor) -> Value {
    json!({
        "id": region.id,
        "label": region.label,
        "kind": memory_region_kind_label(region.kind),
        "size": region.size,
        "address_bits": region.address_bits,
        "readable": region.readable,
        "writable": region.writable,
        "side_effect_free": region.side_effect_free,
        "copyable": region.copyable,
        "view": memory_region_view_label(region.view),
        "aliases": region.aliases,
    })
}

fn memory_region_kind_label(kind: MemoryRegionKind) -> &'static str {
    match kind {
        MemoryRegionKind::CpuAddressSpace => "cpu_address_space",
        MemoryRegionKind::SystemRam => "system_ram",
        MemoryRegionKind::ExternalWorkRam => "external_work_ram",
        MemoryRegionKind::InternalWorkRam => "internal_work_ram",
        MemoryRegionKind::VideoRam => "video_ram",
        MemoryRegionKind::PaletteRam => "palette_ram",
        MemoryRegionKind::Oam => "oam",
        MemoryRegionKind::IoRegisters => "io_registers",
        MemoryRegionKind::SaveRam => "save_ram",
        MemoryRegionKind::Framebuffer => "framebuffer",
    }
}

fn memory_region_view_label(view: MemoryRegionView) -> &'static str {
    match view {
        MemoryRegionView::AddressSpace => "address_space",
        MemoryRegionView::Physical => "physical",
        MemoryRegionView::Aggregate => "aggregate",
        MemoryRegionView::Derived => "derived",
    }
}

fn save_ram_kind_json(kind: SaveRamKind) -> Value {
    match kind {
        SaveRamKind::None => json!({
            "kind": "none",
            "size": 0,
        }),
        SaveRamKind::KnownVolatile { size } => json!({
            "kind": "known_volatile",
            "size": size,
        }),
        SaveRamKind::KnownBatteryBacked { size } => json!({
            "kind": "known_battery_backed",
            "size": size,
        }),
        SaveRamKind::MapperRamUnknown { size } => json!({
            "kind": "mapper_ram_unknown",
            "size": size,
        }),
    }
}

fn set_remote_input(
    host_input: &mut crate::app::input::HostInputState,
    player: u8,
    input: LiveInput,
    pressed: bool,
) {
    match input {
        LiveInput::Button(key) => match player {
            2 => host_input.set_remote_p2(key, pressed),
            3 => host_input.set_remote_p3(key, pressed),
            4 => host_input.set_remote_p4(key, pressed),
            5 => host_input.set_remote_p5(key, pressed),
            _ => host_input.set_remote(key, pressed),
        },
        LiveInput::ColecoKeypad(key) => host_input.set_coleco_remote_keypad(player, key, pressed),
    }
}

fn same_pending_input(release: &PendingButtonRelease, player: u8, input: LiveInput) -> bool {
    release.player == player && release.input == input
}

fn pending_release_frame_limit(releases: &[PendingButtonRelease], frames: usize) -> usize {
    releases
        .iter()
        .map(|release| release.frames_remaining)
        .min()
        .map_or(frames, |remaining| frames.min(remaining))
}

fn advance_pending_button_releases(
    releases: &mut Vec<PendingButtonRelease>,
    frames: usize,
) -> Vec<(u8, LiveInput)> {
    let mut expired = Vec::new();
    for release in releases.iter_mut() {
        release.frames_remaining = release.frames_remaining.saturating_sub(frames);
        if release.frames_remaining == 0 {
            expired.push((release.player, release.input));
        }
    }
    releases.retain(|release| release.frames_remaining > 0);
    expired
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_button_release_limits_frame_batches() {
        let mut releases = vec![
            PendingButtonRelease {
                player: 1,
                input: LiveInput::Button(HostButton::A),
                frames_remaining: 3,
            },
            PendingButtonRelease {
                player: 2,
                input: LiveInput::ColecoKeypad(11),
                frames_remaining: 1,
            },
        ];

        assert_eq!(pending_release_frame_limit(&releases, 4), 1);
        assert_eq!(pending_release_frame_limit(&[], 4), 4);
        assert_eq!(
            advance_pending_button_releases(&mut releases, 1),
            vec![(2, LiveInput::ColecoKeypad(11))]
        );
        assert_eq!(releases[0].frames_remaining, 2);
        assert_eq!(
            advance_pending_button_releases(&mut releases, 2),
            vec![(1, LiveInput::Button(HostButton::A))]
        );
        assert!(releases.is_empty());
    }

    #[test]
    fn core_features_json_exposes_opcode_history_support() {
        let features = CoreCapabilities {
            core_family: CoreFamily::Sega8,
            save_ram_kind: SaveRamKind::MapperRamUnknown { size: 0x8000 },
            has_battery: false,
            system_ram_len: 0x2000,
            video_ram_len: 0x4000,
            memory_regions: Vec::new(),
            input_features: crate::emu_backend::InputCapabilities::for_system(
                crate::emu_backend::ActiveSystem::MasterSystem,
            ),
            cheat_features: crate::emu_backend::CheatCapabilities::for_system(
                crate::emu_backend::ActiveSystem::MasterSystem,
            ),
            supports_save_states: true,
            supports_state_capture: true,
            supports_rewind: true,
            supports_replay: true,
            supports_audio: true,
            supports_cheats: true,
            supports_guest_calls: true,
            supports_debugger: true,
            supports_execution_controls: true,
            supports_opcode_history: true,
            tas_execution_primitives: crate::emu_backend::capabilities::TasCoreCapabilityProbe {
                system_identity_observed: true,
                source_media_identity: None,
                source_media_identity_observed: false,
                effective_media_identity_observed: true,
                firmware_identity_observed: false,
                direct_runtime_profile_requirements_match: false,
                supports_state_restore: true,
                persistent_state:
                    crate::emu_backend::capabilities::TasPersistentStateIdentity::Unknown {
                        size: 0x8000,
                    },
                input_model: crate::emu_backend::capabilities::TasInputModel::StandardDigitalPads {
                    max_players: 2,
                },
            },
        };

        let json = core_features_json(&features);

        assert_eq!(json["core_family"], "sega8");
        assert_eq!(json["supports_debugger"], true);
        assert_eq!(json["supports_execution_controls"], true);
        assert_eq!(json["supports_opcode_history"], true);
        assert_eq!(json["supports_save_states"], true);
        assert_eq!(json["supports_state_capture"], true);
        assert_eq!(json["supports_rewind"], true);
        assert_eq!(json["supports_replay"], true);
        assert_eq!(json["supports_audio"], true);
        assert_eq!(json["supports_cheats"], true);
        assert_eq!(json["supports_guest_calls"], true);
        assert_eq!(json["input"]["supports_player_two"], true);
        assert_eq!(json["input"]["supports_lightgun"], false);
        assert_eq!(json["input"]["buttons"][0], "Up");
        assert_eq!(
            json["tas_execution_primitives"]["persistent_state"]["kind"],
            "unknown"
        );
        assert_eq!(
            json["tas_execution_primitives"]["input_model"]["kind"],
            "standard_digital_pads"
        );
        assert_eq!(
            json["tas_execution_primitives"]["source_media_identity_observed"],
            false
        );
        assert!(json["tas_execution_primitives"]["source_media_identity"].is_null());
        assert_eq!(json["cheats"]["supports_user_cheats"], true);
        assert_eq!(json["cheats"]["supports_rom_patches"], true);
        assert_eq!(json["cheats"]["formats"][1], "Action Replay");
    }

    #[test]
    fn replay_stop_channel_failure_is_not_reported_as_success() {
        let reply = replay_stop_live_reply(Err(EmuCommandSendError::ChannelClosed), || {
            panic!("failure must not build a success status")
        });
        match reply {
            LiveReply::Error(message) => assert!(message.contains("channel is closed")),
            LiveReply::Ok(_) => panic!("channel failure was reported as success"),
        }
    }

    #[test]
    fn memory_region_json_exposes_view_and_access_metadata() {
        let region = MemoryRegionDescriptor::cpu_address_space(20);

        let json = memory_region_json(&region);

        assert_eq!(json["id"], "cpu");
        assert_eq!(json["view"], "address_space");
        assert_eq!(json["readable"], true);
        assert_eq!(json["writable"], true);
        assert_eq!(json["side_effect_free"], true);
        assert_eq!(json["copyable"], false);
    }
}
