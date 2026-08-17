use crate::debug::DebugUiActions;
use crate::emu_core_trait::DebuggableEmulator;
use crate::settings::{DmgPalettePreset, NesPaletteMode};
use zeff_emu_common::address::Address;
use zeff_emu_common::debug::WatchType;
use zeff_nes_core::hardware::ppu::NesPalette;

use super::EmuBackend;

pub(crate) struct BackendRuntimeConfig<'a> {
    pub(crate) debug_actions: &'a DebugUiActions,
    pub(crate) opcode_log_enabled: bool,
    pub(crate) debug_continue: bool,
    pub(crate) debug_step: bool,
    pub(crate) uncapped_mode: bool,
    pub(crate) apu_capture_enabled: bool,
    pub(crate) skip_audio: bool,
    pub(crate) host_tilt: (f32, f32),
    pub(crate) host_camera_frame: Option<&'a [u8]>,
    pub(crate) dmg_palette_preset: DmgPalettePreset,
    pub(crate) sgb_border_enabled: bool,
    pub(crate) nes_palette_mode: NesPaletteMode,
    pub(crate) nes_custom_palette: Option<&'a NesPalette>,
}

impl<'a> BackendRuntimeConfig<'a> {
    pub(crate) fn new(debug_actions: &'a DebugUiActions) -> Self {
        Self {
            debug_actions,
            opcode_log_enabled: false,
            debug_continue: false,
            debug_step: false,
            uncapped_mode: false,
            apu_capture_enabled: false,
            skip_audio: false,
            host_tilt: (0.0, 0.0),
            host_camera_frame: None,
            dmg_palette_preset: DmgPalettePreset::default(),
            sgb_border_enabled: false,
            nes_palette_mode: NesPaletteMode::default(),
            nes_custom_palette: None,
        }
    }
}

enum DebugAction {
    AddBreakpoint(Address),
    AddOneShotBreakpoint(Address),
    AddWatchpoint(Address, Address, WatchType),
    RemoveWatchpoint(Address, Address, WatchType),
    RemoveBreakpoint(Address),
    ToggleBreakpoint(Address),
    WriteMemory(Address, u8),
}

impl EmuBackend {
    pub(crate) fn apply_runtime_config(&mut self, config: BackendRuntimeConfig<'_>) {
        if let Some(mutes) = &config.debug_actions.apu_channel_mutes {
            self.set_apu_channel_mutes(mutes);
        }

        match self {
            Self::Gb(gb) => {
                apply_gb_debug_actions(&mut gb.emu, config.debug_actions);
                gb.emu
                    .set_mbc7_host_tilt(config.host_tilt.0, config.host_tilt.1);
                gb.emu.set_dmg_palette_preset(config.dmg_palette_preset);
                gb.emu.set_sgb_border_enabled(config.sgb_border_enabled);
                if let Some(frame) = config.host_camera_frame {
                    gb.emu.set_camera_host_frame(frame);
                }
                gb.emu
                    .set_apu_debug_capture_enabled(config.apu_capture_enabled);
                if !config.uncapped_mode {
                    gb.emu.set_apu_sample_generation_enabled(!config.skip_audio);
                }
                apply_debug_controls(&mut gb.emu, &config);
            }
            Self::Nes(nes) => {
                apply_debug_actions_to(&mut nes.emu, config.debug_actions);
                nes.emu
                    .set_custom_palette(config.nes_custom_palette.cloned());
                nes.emu.set_palette_mode(config.nes_palette_mode);
                nes.emu
                    .set_apu_debug_collection_enabled(config.apu_capture_enabled);
                apply_debug_controls(&mut nes.emu, &config);
            }
            Self::Gba(gba) => {
                apply_gba_debug_actions(&mut gba.emu, config.debug_actions);
                gba.emu
                    .set_apu_debug_capture_enabled(config.apu_capture_enabled);
                if !config.uncapped_mode {
                    gba.emu
                        .set_apu_sample_generation_enabled(!config.skip_audio);
                }
                apply_debug_controls(&mut gba.emu, &config);
            }
            Self::Ws(ws) => {
                apply_debug_actions_to(&mut ws.emu, config.debug_actions);
                if !config.uncapped_mode {
                    ws.emu.set_apu_sample_generation_enabled(!config.skip_audio);
                }
                apply_debug_controls(&mut ws.emu, &config);
            }
            Self::Sega8(sega8) => {
                apply_debug_actions_to(&mut sega8.emu, config.debug_actions);
                if !config.uncapped_mode {
                    sega8
                        .emu
                        .set_apu_sample_generation_enabled(!config.skip_audio);
                }
                apply_debug_controls(&mut sega8.emu, &config);
            }
        }
    }
}

fn collect_debug_actions(actions: &DebugUiActions) -> impl Iterator<Item = DebugAction> + '_ {
    actions
        .add_breakpoint
        .iter()
        .map(|&addr| DebugAction::AddBreakpoint(addr))
        .chain(
            actions
                .add_one_shot_breakpoint
                .iter()
                .map(|&addr| DebugAction::AddOneShotBreakpoint(addr)),
        )
        .chain(
            actions
                .add_watchpoint
                .iter()
                .map(|&(start, end, wt)| DebugAction::AddWatchpoint(start, end, wt)),
        )
        .chain(
            actions
                .remove_watchpoints
                .iter()
                .map(|&(start, end, wt)| DebugAction::RemoveWatchpoint(start, end, wt)),
        )
        .chain(
            actions
                .remove_breakpoints
                .iter()
                .map(|&addr| DebugAction::RemoveBreakpoint(addr)),
        )
        .chain(
            actions
                .toggle_breakpoints
                .iter()
                .map(|&addr| DebugAction::ToggleBreakpoint(addr)),
        )
        .chain(
            actions
                .memory_writes
                .iter()
                .map(|&(addr, value)| DebugAction::WriteMemory(addr, value)),
        )
}

fn apply_debug_actions_to(emu: &mut impl DebuggableEmulator, actions: &DebugUiActions) {
    for action in collect_debug_actions(actions) {
        match action {
            DebugAction::AddBreakpoint(addr) => emu.add_breakpoint(addr),
            DebugAction::AddOneShotBreakpoint(addr) => emu.add_one_shot_breakpoint(addr),
            DebugAction::AddWatchpoint(start, end, wt) => emu.add_watchpoint_range(start, end, wt),
            DebugAction::RemoveWatchpoint(start, end, wt) => emu.remove_watchpoint(start, end, wt),
            DebugAction::RemoveBreakpoint(addr) => emu.remove_breakpoint(addr),
            DebugAction::ToggleBreakpoint(addr) => emu.toggle_breakpoint(addr),
            DebugAction::WriteMemory(addr, val) => emu.cpu_write8(addr, val),
        }
    }
}

fn apply_gb_debug_actions(emu: &mut zeff_gb_core::emulator::Emulator, actions: &DebugUiActions) {
    apply_debug_actions_to(emu, actions);
    for &offset in &actions.add_rom_breakpoints {
        if let Ok(offset) = usize::try_from(offset) {
            emu.add_rom_breakpoint(offset);
        }
    }
    for &offset in &actions.remove_rom_breakpoints {
        if let Ok(offset) = usize::try_from(offset) {
            emu.remove_rom_breakpoint(offset);
        }
    }
    for &offset in &actions.toggle_rom_breakpoints {
        if let Ok(offset) = usize::try_from(offset) {
            emu.toggle_rom_breakpoint(offset);
        }
    }
    if let Some((bg, win, sprites)) = actions.layer_toggles {
        emu.set_ppu_debug_flags(bg, win, sprites);
    }
}

fn apply_gba_debug_actions(emu: &mut zeff_gba_core::emulator::Emulator, actions: &DebugUiActions) {
    apply_debug_actions_to(emu, actions);
    if let Some((bg, win, sprites)) = actions.layer_toggles {
        emu.set_ppu_debug_flags(bg, win, sprites);
    }
    if let Some(layers) = actions.gba_bg_layer_toggles {
        emu.set_ppu_debug_bg_layers(layers);
    }
}

fn apply_debug_controls(emu: &mut impl DebuggableEmulator, config: &BackendRuntimeConfig<'_>) {
    if emu.supports_opcode_history() {
        emu.set_opcode_log_enabled(config.opcode_log_enabled);
    }
    if emu.is_cpu_suspended() {
        if config.debug_continue {
            emu.debug_continue();
        } else if config.debug_step {
            emu.debug_step();
        }
    }
}
