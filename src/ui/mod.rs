use crate::debug::{
    DebugInfo, DebugUiActions, DebugViewerData, DisassemblyView, RomInfoViewData,
    disassemble_around,
};
use crate::emu_thread::SnapshotRequest;
use crate::emulator::Emulator;
use crate::hardware::types::hardware_mode::HardwareMode;

pub(crate) struct UiFrameData {
    pub(crate) debug_info: Option<DebugInfo>,
    pub(crate) viewer_data: Option<DebugViewerData>,
    pub(crate) disassembly_view: Option<DisassemblyView>,
    pub(crate) rom_info_view: Option<RomInfoViewData>,
    pub(crate) memory_page: Option<Vec<(u16, u8)>>,
}

pub(crate) fn collect_emu_snapshot(
    emu: &Emulator,
    req: &SnapshotRequest,
) -> UiFrameData {
    let debug_info = if req.want_debug_info {
        Some(emu.snapshot())
    } else {
        None
    };

    let viewer_data = if req.any_viewer_open {
        let ppu = emu.ppu_registers();
        let cgb_mode = matches!(
            emu.hardware_mode,
            HardwareMode::CGBNormal | HardwareMode::CGBDouble
        );
        let bg_palette_ram = emu.bus.io.ppu.bg_palette_ram;
        let obj_palette_ram = emu.bus.io.ppu.obj_palette_ram;
        let vram = if req.any_vram_viewer_open {
            emu.vram().to_vec()
        } else {
            Vec::new()
        };

        Some(DebugViewerData {
            vram,
            oam: if req.show_oam_viewer {
                emu.oam().to_vec()
            } else {
                Vec::new()
            },
            apu_regs: if req.show_apu_viewer {
                emu.bus.io.apu.regs_snapshot()
            } else {
                [0; 0x17]
            },
            apu_wave_ram: if req.show_apu_viewer {
                emu.bus.io.apu.wave_ram_snapshot()
            } else {
                [0; 0x10]
            },
            apu_nr52: if req.show_apu_viewer {
                emu.bus.io.apu.nr52_raw()
            } else {
                0
            },
            apu_channel_samples: if req.show_apu_viewer {
                [
                    emu.bus.io.apu.channel_debug_samples_ordered(0),
                    emu.bus.io.apu.channel_debug_samples_ordered(1),
                    emu.bus.io.apu.channel_debug_samples_ordered(2),
                    emu.bus.io.apu.channel_debug_samples_ordered(3),
                ]
            } else {
                [[0.0; 512]; 4]
            },
            apu_master_samples: if req.show_apu_viewer {
                emu.bus.io.apu.master_debug_samples_ordered()
            } else {
                [0.0; 512]
            },
            apu_channel_muted: if req.show_apu_viewer {
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

    let disassembly_view = if req.show_disassembler {
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

    let rom_info_view = if req.show_rom_info {
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

    let memory_page = if req.show_memory_viewer {
        Some(emu.read_memory_range(req.memory_view_start, 256))
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
    actions: &DebugUiActions,
    debug_step_requested: &mut bool,
    debug_continue_requested: &mut bool,
) {
    if actions.step_requested {
        *debug_step_requested = true;
    }
    if actions.continue_requested {
        *debug_continue_requested = true;
    }
}
