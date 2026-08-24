use crate::debug::DebugUiActions;
use crate::emu_core_trait::{DebuggableEmulator, EmulatorCore};
use crate::settings::{DmgPalettePreset, NesPaletteMode, PceOverscanMode, PcePaletteMode};
use zeff_emu_common::address::Address;
use zeff_emu_common::debug::WatchType;
use zeff_nes_core::hardware::ppu::NesPalette;

use super::EmuBackend;
use crate::emu_thread::GuestCallRequest;

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
    pub(crate) pce_overscan_mode: PceOverscanMode,
    pub(crate) pce_palette_mode: PcePaletteMode,
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
            pce_overscan_mode: PceOverscanMode::default(),
            pce_palette_mode: PcePaletteMode::default(),
        }
    }
}

enum DebugAction {
    AddBreakpoint(Address),
    AddOneShotBreakpoint(Address),
    AddBreakpointAfter(Address, u64),
    SetEventBreakpoint(zeff_emu_common::debug::DebugEvent, bool),
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
        let supports_debugger = self.supports_debugger();
        let supports_execution_controls = self.supports_execution_controls();

        match self {
            Self::Gb(gb) => {
                if supports_debugger {
                    apply_gb_debug_actions(&mut gb.emu, config.debug_actions);
                }
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
                if supports_debugger {
                    apply_debug_controls(&mut gb.emu, &config);
                }
            }
            Self::Nes(nes) => {
                if supports_debugger {
                    apply_debug_actions_to(&mut nes.emu, config.debug_actions);
                }
                nes.emu
                    .set_custom_palette(config.nes_custom_palette.cloned());
                nes.emu.set_palette_mode(config.nes_palette_mode);
                nes.emu
                    .set_apu_debug_collection_enabled(config.apu_capture_enabled);
                if supports_debugger {
                    apply_debug_controls(&mut nes.emu, &config);
                }
            }
            Self::Pce(pce) => {
                pce.set_display_config(config.pce_overscan_mode, config.pce_palette_mode);
                pce.set_apu_debug_capture_enabled(config.apu_capture_enabled);
                if !config.uncapped_mode {
                    pce.set_apu_sample_generation_enabled(!config.skip_audio);
                }
                if supports_debugger {
                    apply_debug_actions_to(pce.as_mut(), config.debug_actions);
                }
                if supports_execution_controls {
                    apply_debug_controls(pce.as_mut(), &config);
                }
            }
            Self::Gba(gba) => {
                if supports_debugger {
                    apply_gba_debug_actions(&mut gba.emu, config.debug_actions);
                }
                gba.emu
                    .set_apu_debug_capture_enabled(config.apu_capture_enabled);
                if !config.uncapped_mode {
                    gba.emu
                        .set_apu_sample_generation_enabled(!config.skip_audio);
                }
                if supports_debugger {
                    apply_debug_controls(&mut gba.emu, &config);
                }
            }
            Self::Ws(ws) => {
                if supports_debugger {
                    apply_debug_actions_to(&mut ws.emu, config.debug_actions);
                }
                if !config.uncapped_mode {
                    ws.emu.set_apu_sample_generation_enabled(!config.skip_audio);
                }
                if supports_debugger {
                    apply_debug_controls(&mut ws.emu, &config);
                }
            }
            Self::Sega8(sega8) => {
                if supports_debugger {
                    apply_debug_actions_to(&mut sega8.emu, config.debug_actions);
                }
                if !config.uncapped_mode {
                    sega8
                        .emu
                        .set_apu_sample_generation_enabled(!config.skip_audio);
                }
                if supports_debugger {
                    apply_debug_controls(&mut sega8.emu, &config);
                }
            }
        }
    }

    pub(crate) fn execute_guest_call(
        &mut self,
        request: &GuestCallRequest,
    ) -> anyhow::Result<(u64, Vec<u8>)> {
        anyhow::ensure!(
            self.supports_guest_calls(),
            "guest calls are not supported by this core"
        );
        anyhow::ensure!(self.is_suspended(), "CPU must be suspended");
        self.validate_guest_call_target(request)?;
        let saved = self.encode_state_bytes()?;
        let result = match self {
            Self::Gb(backend) => {
                let target = u16::try_from(request.target)
                    .map_err(|_| anyhow::anyhow!("target is out of range"))?;
                anyhow::ensure!(
                    request.exec_mode == crate::symbols::ExecMode::Sm83,
                    "expected SM83 code"
                );
                backend
                    .emu
                    .debug_execute_guest_call(target, request.instruction_budget)
            }
            Self::Nes(backend) => {
                let target = u16::try_from(request.target)
                    .map_err(|_| anyhow::anyhow!("target is out of range"))?;
                anyhow::ensure!(
                    request.exec_mode == crate::symbols::ExecMode::Mos6502,
                    "expected 6502 code"
                );
                backend
                    .emu
                    .debug_execute_guest_call(target, request.instruction_budget)
            }
            Self::Pce(backend) => {
                let target = u16::try_from(request.target)
                    .map_err(|_| anyhow::anyhow!("target is out of range"))?;
                anyhow::ensure!(
                    request.exec_mode == crate::symbols::ExecMode::HuC6280,
                    "expected HuC6280 code"
                );
                backend.debug_execute_guest_call(target, request.instruction_budget)
            }
            Self::Sega8(backend) => {
                let target = u16::try_from(request.target)
                    .map_err(|_| anyhow::anyhow!("target is out of range"))?;
                anyhow::ensure!(
                    request.exec_mode == crate::symbols::ExecMode::Z80,
                    "expected Z80 code"
                );
                backend
                    .emu
                    .debug_execute_guest_call(target, request.instruction_budget)
            }
            Self::Gba(backend) => {
                let thumb = match request.exec_mode {
                    crate::symbols::ExecMode::Arm => false,
                    crate::symbols::ExecMode::Thumb => true,
                    _ => anyhow::bail!("expected ARM or Thumb code"),
                };
                backend.emu.debug_execute_guest_call(
                    request.target,
                    thumb,
                    request.instruction_budget,
                )
            }
            Self::Ws(backend) => {
                anyhow::ensure!(request.target <= 0x000F_FFFF, "target is out of range");
                anyhow::ensure!(
                    request.exec_mode == crate::symbols::ExecMode::V30,
                    "expected V30 code"
                );
                backend
                    .emu
                    .debug_execute_guest_call(request.target, request.instruction_budget)
            }
        };
        match result {
            Ok(instructions) => Ok((instructions, saved)),
            Err(error) => {
                self.load_state_from_bytes(saved)
                    .map_err(|restore| anyhow::anyhow!("{error}; rollback failed: {restore}"))?;
                anyhow::bail!("{error}; state restored")
            }
        }
    }

    fn validate_guest_call_target(&self, request: &GuestCallRequest) -> anyhow::Result<()> {
        let Some(expected) = request.storage_offset else {
            return Ok(());
        };
        if request.explicit_overlay {
            return Ok(());
        }
        let actual = match self {
            Self::Gb(backend) => u16::try_from(request.target)
                .ok()
                .and_then(|target| backend.emu.rom_offset_for_cpu_address(target)),
            Self::Nes(backend) => u16::try_from(request.target)
                .ok()
                .and_then(|target| backend.emu.rom_offset_for_cpu_address(target)),
            Self::Pce(backend) => u16::try_from(request.target)
                .ok()
                .and_then(|target| backend.rom_offset_for_cpu_address(target))
                .map(|offset| offset as usize),
            Self::Sega8(backend) => u16::try_from(request.target)
                .ok()
                .and_then(|target| backend.emu.rom_offset_for_cpu_address(target)),
            Self::Gba(backend) => backend.emu.rom_offset_for_cpu_address(request.target),
            Self::Ws(backend) => backend.emu.rom_offset_for_cpu_address(request.target),
        }
        .map(|offset| offset as u64);
        anyhow::ensure!(
            actual == Some(expected),
            "target no longer maps to ROM offset {expected:X}"
        );
        Ok(())
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
                .add_breakpoint_after
                .iter()
                .map(|&(addr, hits)| DebugAction::AddBreakpointAfter(addr, hits)),
        )
        .chain(
            actions
                .event_breakpoint_changes
                .iter()
                .map(|&(event, enabled)| DebugAction::SetEventBreakpoint(event, enabled)),
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
            DebugAction::AddBreakpointAfter(addr, hits) => emu.add_breakpoint_after(addr, hits),
            DebugAction::SetEventBreakpoint(event, enabled) => {
                emu.set_event_breakpoint(event, enabled)
            }
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
    if let Some(enabled) = config.debug_actions.trace_enabled {
        emu.set_instruction_trace_enabled(enabled);
    }
    if let Some(capacity) = config.debug_actions.trace_capacity {
        emu.set_instruction_trace_capacity(capacity);
    }
    if config.debug_actions.trace_clear {
        emu.clear_instruction_trace();
    }
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
