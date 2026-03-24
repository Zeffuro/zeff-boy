use crate::debug::{
    DebugInfo, DebugUiActions, DebugViewerData, DisassemblyView, MemorySearchResult,
    RomInfoViewData, RomSearchResult, disassemble_around,
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
    pub(crate) memory_search_results: Option<Vec<MemorySearchResult>>,
    pub(crate) rom_page: Option<Vec<(u32, u8)>>,
    pub(crate) rom_size: u32,
    pub(crate) rom_search_results: Option<Vec<RomSearchResult>>,
}

pub(crate) fn compute_static_rom_info(emu: &Emulator) -> RomInfoViewData {
    let header = emu.rom_info();
    let rom_bytes = emu.bus.cartridge.rom_bytes();
    let rom_crc32 = crc32fast::hash(rom_bytes);
    let is_gbc = header.is_cgb_compatible || header.is_cgb_exclusive;
    let libretro_meta = crate::libretro_metadata::lookup_cached(rom_crc32, is_gbc);
    let manufacturer = header
        .manufacturer_code
        .as_deref()
        .unwrap_or("N/A")
        .to_string();
    RomInfoViewData {
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
        rom_crc32,
        libretro_title: libretro_meta.as_ref().map(|m| m.title.clone()),
        libretro_rom_name: libretro_meta.as_ref().map(|m| m.rom_name.clone()),
        hardware_mode: emu.hardware_mode,
        cartridge_state: emu.cartridge_state(),
    }
}

pub(crate) fn collect_emu_snapshot(
    emu: &Emulator,
    req: &SnapshotRequest,
    cached_rom_info: &Option<RomInfoViewData>,
    reusable_vram: Option<Vec<u8>>,
    reusable_oam: Option<Vec<u8>>,
    reusable_memory_page: Option<Vec<(u16, u8)>>,
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
            let src = emu.vram();
            let mut buf = reusable_vram.unwrap_or_default();
            buf.resize(src.len(), 0);
            buf.copy_from_slice(src);
            buf
        } else {
            reusable_vram
                .map(|mut v| {
                    v.clear();
                    v
                })
                .unwrap_or_default()
        };

        Some(DebugViewerData {
            vram,
            oam: if req.show_oam_viewer {
                let src = emu.oam();
                let mut buf = reusable_oam.unwrap_or_default();
                buf.resize(src.len(), 0);
                buf.copy_from_slice(src);
                buf
            } else {
                reusable_oam
                    .map(|mut v| {
                        v.clear();
                        v
                    })
                    .unwrap_or_default()
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
        let pc_changed = req
            .last_disasm_pc
            .map_or(true, |last_pc| last_pc != emu.cpu.pc);
        if pc_changed {
            let mut breakpoints: Vec<u16> = emu.debug.breakpoints.iter().copied().collect();
            breakpoints.sort_unstable();
            Some(DisassemblyView {
                pc: emu.cpu.pc,
                lines: disassemble_around(|addr| emu.bus.read_byte(addr), emu.cpu.pc, 12, 26),
                breakpoints,
            })
        } else {
            None
        }
    } else {
        None
    };

    let rom_info_view = if req.show_rom_info {
        cached_rom_info.as_ref().map(|cached| {
            let mut info = cached.clone();
            // Only update the dynamic fields
            info.hardware_mode = emu.hardware_mode;
            info.cartridge_state = emu.cartridge_state();
            if info.libretro_title.is_none() {
                let is_gbc = info.is_cgb_compatible || info.is_cgb_exclusive;
                if let Some(meta) = crate::libretro_metadata::lookup_cached(info.rom_crc32, is_gbc)
                {
                    info.libretro_title = Some(meta.title);
                    info.libretro_rom_name = Some(meta.rom_name);
                }
            }
            info
        })
    } else {
        None
    };

    let memory_page = if req.show_memory_viewer {
        let mut buf = reusable_memory_page.unwrap_or_else(|| Vec::with_capacity(256));
        buf.clear();
        let start = req.memory_view_start;
        for i in 0..256u16 {
            let addr = start.wrapping_add(i);
            buf.push((addr, emu.bus.read_byte(addr)));
        }
        Some(buf)
    } else {
        // Keep the buffer alive for reuse even when not needed
        reusable_memory_page.map(|mut v| {
            v.clear();
            v
        })
    };

    let memory_search_results = if let Some(ref search) = req.memory_search {
        let mut results = Vec::new();
        if !search.pattern.is_empty() {
            let pattern_len = search.pattern.len();
            for start_addr in 0u32..=0xFFFFu32 {
                if results.len() >= search.max_results {
                    break;
                }
                let mut matched = true;
                for (offset, &expected) in search.pattern.iter().enumerate() {
                    let addr = (start_addr as u16).wrapping_add(offset as u16);
                    if emu.bus.read_byte(addr) != expected {
                        matched = false;
                        break;
                    }
                }
                if matched {
                    let matched_bytes: Vec<u8> = (0..pattern_len)
                        .map(|offset| {
                            emu.bus
                                .read_byte((start_addr as u16).wrapping_add(offset as u16))
                        })
                        .collect();
                    results.push(MemorySearchResult {
                        address: start_addr as u16,
                        matched_bytes,
                    });
                }
            }
        }
        Some(results)
    } else {
        None
    };

    let rom_bytes = emu.bus.cartridge.rom_bytes();
    let rom_size = rom_bytes.len() as u32;

    let rom_page = if req.show_rom_viewer {
        let start = req.rom_view_start as usize;
        let mut buf = Vec::with_capacity(256);
        for i in 0..256usize {
            let offset = start + i;
            if offset < rom_bytes.len() {
                buf.push((offset as u32, rom_bytes[offset]));
            }
        }
        Some(buf)
    } else {
        None
    };

    let rom_search_results = if let Some(ref search) = req.rom_search {
        let mut results = Vec::new();
        if !search.pattern.is_empty() {
            let pattern_len = search.pattern.len();
            let end = rom_bytes
                .len()
                .saturating_sub(pattern_len.saturating_sub(1));
            for start_offset in 0..end {
                if results.len() >= search.max_results {
                    break;
                }
                if rom_bytes[start_offset..start_offset + pattern_len] == search.pattern[..] {
                    results.push(RomSearchResult {
                        offset: start_offset as u32,
                        matched_bytes: rom_bytes[start_offset..start_offset + pattern_len].to_vec(),
                    });
                }
            }
        }
        Some(results)
    } else {
        None
    };

    UiFrameData {
        debug_info,
        viewer_data,
        disassembly_view,
        rom_info_view,
        memory_page,
        memory_search_results,
        rom_page,
        rom_size,
        rom_search_results,
    }
}

pub(crate) fn apply_debug_actions(
    actions: &DebugUiActions,
    debug_step_requested: &mut bool,
    debug_continue_requested: &mut bool,
    backstep_requested: &mut bool,
) {
    if actions.step_requested {
        *debug_step_requested = true;
    }
    if actions.continue_requested {
        *debug_continue_requested = true;
    }
    if actions.backstep_requested {
        *backstep_requested = true;
    }
}
