use std::sync::{Arc, Mutex};

use crate::debug::{
    DebugInfo, DebugUiActions, DebugViewerData, DebugWindowState, DisassemblyView, RomInfoViewData,
    disassemble_around,
};
use crate::emulator::Emulator;
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::settings::Settings;

#[derive(Clone, Copy)]
pub(crate) struct UiTiltFrameData {
    pub(crate) is_mbc7: bool,
    pub(crate) stick_controls_tilt: bool,
    pub(crate) keyboard: (f32, f32),
    pub(crate) mouse: (f32, f32),
    pub(crate) left_stick: (f32, f32),
    pub(crate) target: (f32, f32),
    pub(crate) smoothed: (f32, f32),
}

pub(crate) struct UiFrameData {
    pub(crate) debug_info: Option<DebugInfo>,
    pub(crate) viewer_data: Option<DebugViewerData>,
    pub(crate) disassembly_view: Option<DisassemblyView>,
    pub(crate) rom_info_view: Option<RomInfoViewData>,
    pub(crate) memory_page: Option<Vec<(u16, u8)>>,
}

pub(crate) fn collect_ui_frame_data(
    emu: &Emulator,
    debug_windows: &mut DebugWindowState,
    settings: &Settings,
    tilt_data: UiTiltFrameData,
    fps: f64,
    speed_mode_label: &'static str,
) -> UiFrameData {
    let any_viewer_open = debug_windows.any_viewer_open();
    let any_vram_viewer_open = debug_windows.any_vram_viewer_open();
    let show_apu_viewer = debug_windows.show_apu_viewer;
    let show_disassembler = debug_windows.show_disassembler;
    let show_rom_info = debug_windows.show_rom_info;
    let show_memory_viewer = debug_windows.show_memory_viewer;

    let debug_info = {
        let mut info = emu.snapshot();
        info.fps = if settings.show_fps { fps } else { 0.0 };
        info.speed_mode_label = speed_mode_label;
        info.tilt_is_mbc7 = tilt_data.is_mbc7;
        info.tilt_stick_controls_tilt = tilt_data.stick_controls_tilt;
        info.tilt_left_stick = tilt_data.left_stick;
        info.tilt_keyboard = tilt_data.keyboard;
        info.tilt_mouse = tilt_data.mouse;
        info.tilt_target = tilt_data.target;
        info.tilt_smoothed = tilt_data.smoothed;
        Some(info)
    };

    let viewer_data = if any_viewer_open {
        let ppu = emu.ppu_registers();
        let cgb_mode = matches!(
            emu.hardware_mode,
            HardwareMode::CGBNormal | HardwareMode::CGBDouble
        );
        let bg_palette_ram = emu.bus.io.ppu.bg_palette_ram;
        let obj_palette_ram = emu.bus.io.ppu.obj_palette_ram;
        let vram = if any_vram_viewer_open {
            let vram = emu.vram().to_vec();
            if debug_windows.show_tile_viewer {
                debug_windows.update_tile_viewer_dirty_inputs(
                    &vram,
                    &bg_palette_ram,
                    &obj_palette_ram,
                    ppu.bgp,
                    cgb_mode,
                );
            }
            if debug_windows.show_tilemap_viewer {
                debug_windows.update_tilemap_dirty_inputs(&vram, &bg_palette_ram, ppu, cgb_mode);
            }
            vram
        } else {
            Vec::new()
        };

        Some(DebugViewerData {
            vram,
            oam: emu.oam().to_vec(),
            apu_regs: if show_apu_viewer {
                emu.bus.io.apu.regs_snapshot()
            } else {
                [0; 0x17]
            },
            apu_wave_ram: if show_apu_viewer {
                emu.bus.io.apu.wave_ram_snapshot()
            } else {
                [0; 0x10]
            },
            apu_nr52: if show_apu_viewer {
                emu.bus.io.apu.nr52_raw()
            } else {
                0
            },
            apu_channel_samples: if show_apu_viewer {
                [
                    emu.bus.io.apu.channel_debug_samples_ordered(0),
                    emu.bus.io.apu.channel_debug_samples_ordered(1),
                    emu.bus.io.apu.channel_debug_samples_ordered(2),
                    emu.bus.io.apu.channel_debug_samples_ordered(3),
                ]
            } else {
                [[0.0; 512]; 4]
            },
            apu_master_samples: if show_apu_viewer {
                emu.bus.io.apu.master_debug_samples_ordered()
            } else {
                [0.0; 512]
            },
            apu_channel_muted: if show_apu_viewer {
                emu.bus.io.apu.channel_mutes()
            } else {
                [false; 4]
            },
            ppu,
            cgb_mode,
            bg_palette_ram,
            obj_palette_ram,
        })
    } else {
        None
    };

    let disassembly_view = if show_disassembler {
        let mut breakpoints: Vec<u16> = emu.debug.breakpoints.iter().copied().collect();
        breakpoints.sort_unstable();
        Some(DisassemblyView {
            pc: emu.cpu.pc,
            lines: disassemble_around(|addr| emu.bus.read_byte(addr), emu.cpu.pc, 12, 26),
            breakpoints,
        })
    } else {
        None
    };

    let rom_info_view = if show_rom_info {
        let header = emu.rom_info();
        let rom_bytes = emu.bus.cartridge.rom_bytes();
        let manufacturer = header
            .manufacturer_code
            .as_deref()
            .unwrap_or("N/A")
            .to_string();
        Some(RomInfoViewData {
            title: header.title.clone(),
            manufacturer,
            publisher: header.publisher().to_string(),
            cartridge_type: format!("{:?}", header.cartridge_type),
            rom_size: format!("{:?}", header.rom_size),
            ram_size: format!("{:?}", header.ram_size),
            cgb_flag: header.cgb_flag,
            sgb_flag: header.sgb_flag,
            is_cgb_compatible: header.is_cgb_compatible,
            is_cgb_exclusive: header.is_cgb_exclusive,
            is_sgb_supported: header.is_sgb_supported,
            header_checksum_valid: header.verify_header_checksum(rom_bytes),
            global_checksum_valid: header.verify_global_checksum(rom_bytes),
            hardware_mode: emu.hardware_mode,
            cartridge_state: emu.cartridge_state(),
        })
    } else {
        None
    };

    let memory_page = if show_memory_viewer {
        Some(emu.read_memory_range(debug_windows.memory_view_start, 256))
    } else {
        None
    };

    UiFrameData {
        debug_info,
        viewer_data,
        disassembly_view,
        rom_info_view,
        memory_page,
    }
}

pub(crate) fn apply_debug_actions(
    emulator: Option<&Arc<Mutex<Emulator>>>,
    actions: &DebugUiActions,
    debug_step_requested: &mut bool,
    debug_continue_requested: &mut bool,
) {
    if let Some(emu) = emulator {
        let mut emu = emu.lock().expect("emulator mutex poisoned");
        if let Some(addr) = actions.add_breakpoint {
            emu.debug.add_breakpoint(addr);
        }
        if let Some((addr, watch_type)) = actions.add_watchpoint {
            emu.debug.add_watchpoint(addr, watch_type);
        }
        for addr in &actions.remove_breakpoints {
            emu.debug.remove_breakpoint(*addr);
        }
        for addr in &actions.toggle_breakpoints {
            emu.debug.toggle_breakpoint(*addr);
        }
        if let Some(mutes) = actions.apu_channel_mutes {
            emu.bus.io.apu.set_channel_mutes(mutes);
        }
        #[cfg(debug_assertions)]
        for (addr, value) in &actions.memory_writes {
            emu.bus.write_byte(*addr, *value);
        }
    }

    if actions.step_requested {
        *debug_step_requested = true;
    }
    if actions.continue_requested {
        *debug_continue_requested = true;
    }
}
