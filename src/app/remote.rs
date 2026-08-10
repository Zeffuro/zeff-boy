use serde_json::{Value, json};
use zeff_gb_core::hardware::joypad::JoypadKey;

use super::App;
use crate::emu_thread::EmuCommand;
use crate::live_control::{LiveCommand, LiveReply, PendingButtonRelease};
use crate::ui::CoreFeatureInfo;
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::system::CoreFamily;

mod artifacts;
mod graphics;
mod json_helpers;
mod memory;

use json_helpers::{cpu_debug_json, live_speed_mode_name, live_system_name};

impl App {
    pub(super) fn drain_live_control(&mut self) {
        self.update_live_button_releases();

        while let Some(request) = self.live_control.try_recv() {
            request.respond_with(|command| self.handle_live_command(command));
        }
    }

    fn update_live_button_releases(&mut self) {
        for release in &mut self.live_button_releases {
            release.frames_remaining = release.frames_remaining.saturating_sub(1);
            if release.frames_remaining == 0 {
                self.host_input.set_remote(release.key, false);
            }
        }
        self.live_button_releases
            .retain(|release| release.frames_remaining > 0);
    }

    fn handle_live_command(&mut self, command: LiveCommand) -> LiveReply {
        match command {
            LiveCommand::Status => LiveReply::ok(self.live_status_json()),
            LiveCommand::DebugInfo => {
                self.remote_debug_frames_remaining = 3;
                LiveReply::ok(self.live_debug_json())
            }
            LiveCommand::Pause => {
                self.speed.paused = true;
                self.toast_manager.set_paused(true);
                LiveReply::ok(self.live_status_json())
            }
            LiveCommand::Resume => {
                self.speed.paused = false;
                self.timing.last_frame_time = crate::platform::Instant::now();
                self.toast_manager.set_paused(false);
                LiveReply::ok(self.live_status_json())
            }
            LiveCommand::TogglePause => {
                self.speed.paused = !self.speed.paused;
                if !self.speed.paused {
                    self.timing.last_frame_time = crate::platform::Instant::now();
                }
                self.toast_manager.set_paused(self.speed.paused);
                LiveReply::ok(self.live_status_json())
            }
            LiveCommand::FrameAdvance => {
                self.speed.paused = true;
                self.debug_requests.frame_advance = true;
                self.remote_debug_frames_remaining = 3;
                self.toast_manager.set_paused(true);
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
                self.timing.uncapped_speed = enabled;
                self.settings.emulation.uncapped_speed = enabled;
                if let Some(thread) = &self.emu_thread {
                    thread.send(EmuCommand::SetUncapped(enabled));
                }
                LiveReply::ok(self.live_status_json())
            }
            LiveCommand::Button { key, pressed } => {
                self.host_input.set_remote(key, pressed);
                if !pressed {
                    self.live_button_releases
                        .retain(|release| !same_joypad_key(release.key, key));
                }
                LiveReply::ok(self.live_input_json())
            }
            LiveCommand::Tap { key, frames } => {
                self.host_input.set_remote(key, true);
                self.live_button_releases
                    .retain(|release| !same_joypad_key(release.key, key));
                self.live_button_releases.push(PendingButtonRelease {
                    key,
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
            LiveCommand::MemoryRead {
                space,
                start,
                length,
            } => LiveReply::ok(self.live_memory_json(&space, start, length)),
            LiveCommand::GraphicsInfo => LiveReply::ok(self.live_graphics_json()),
        }
    }

    fn live_status_json(&self) -> Value {
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
        json!({
            "buttons": buttons,
            "dpad": dpad,
            "zapper": self.remote_zapper.map(|zapper| json!({
                "enabled": zapper.enabled,
                "trigger": zapper.trigger,
                "hit": zapper.hit,
                "screen_pos": zapper.screen_pos.map(|(x, y)| json!({ "x": x, "y": y })),
            })),
        })
    }
}

fn core_features_json(features: &CoreFeatureInfo) -> Value {
    json!({
        "core_family": core_family_label(features.core_family),
        "save_ram": save_ram_kind_json(features.save_ram_kind),
        "has_battery": features.has_battery,
        "system_ram_len": features.system_ram_len,
        "video_ram_len": features.video_ram_len,
        "memory_regions": features.memory_regions.iter().map(memory_region_json).collect::<Vec<_>>(),
        "supports_save_states": features.supports_save_states,
        "supports_rewind": features.supports_rewind,
        "supports_debugger": features.supports_debugger,
    })
}

fn core_family_label(family: CoreFamily) -> &'static str {
    match family {
        CoreFamily::GameBoy => "game_boy",
        CoreFamily::GameBoyAdvance => "game_boy_advance",
        CoreFamily::Nes => "nes",
        CoreFamily::WonderSwan => "wonder_swan",
        CoreFamily::Sega8 => "sega8",
    }
}

fn memory_region_json(region: &MemoryRegionDescriptor) -> Value {
    json!({
        "id": region.id,
        "label": region.label,
        "kind": memory_region_kind_label(region.kind),
        "size": region.size,
        "address_bits": region.address_bits,
        "writable": region.writable,
        "aliases": region.aliases,
    })
}

fn memory_region_kind_label(kind: MemoryRegionKind) -> &'static str {
    match kind {
        MemoryRegionKind::CpuAddressSpace => "cpu_address_space",
        MemoryRegionKind::SystemRam => "system_ram",
        MemoryRegionKind::VideoRam => "video_ram",
        MemoryRegionKind::PaletteRam => "palette_ram",
        MemoryRegionKind::Oam => "oam",
        MemoryRegionKind::IoRegisters => "io_registers",
        MemoryRegionKind::SaveRam => "save_ram",
        MemoryRegionKind::Framebuffer => "framebuffer",
    }
}

fn save_ram_kind_json(kind: SaveRamKind) -> Value {
    match kind {
        SaveRamKind::None => json!({
            "kind": "none",
            "size": 0,
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

fn same_joypad_key(a: JoypadKey, b: JoypadKey) -> bool {
    std::mem::discriminant(&a) == std::mem::discriminant(&b)
}
