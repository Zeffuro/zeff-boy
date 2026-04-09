mod apu;
mod cpu;
mod input;
mod oam;
mod palette;
mod rom_info;

use super::UiFrameData;
use crate::debug::{
    ConsoleGraphicsData, DisassemblyView, GbGraphicsData, MemorySearchResult, PerfInfo,
    RomSearchResult, disassemble_around,
};
use crate::emu_thread::SnapshotRequest;
use zeff_gb_core::emulator::Emulator;

use apu::gb_apu_snapshot;
use cpu::gb_cpu_snapshot;
use input::gb_input_snapshot;
use oam::gb_oam_snapshot;
use palette::gb_palette_snapshot;
use rom_info::gb_rom_info;

pub(crate) fn collect_emu_snapshot(
    emu: &Emulator,
    req: &SnapshotRequest,
    reusable_vram: Option<Vec<u8>>,
    reusable_oam: Option<Vec<u8>>,
    reusable_memory_page: Option<Vec<(u16, u8)>>,
) -> UiFrameData {
    let gb_info = if req.want_debug_info {
        Some(emu.snapshot())
    } else {
        None
    };

    let cpu_debug = gb_info.as_ref().map(gb_cpu_snapshot);
    let input_debug = gb_info.as_ref().map(gb_input_snapshot);
    let apu_debug = gb_apu_snapshot(emu, req.show_apu_viewer);
    let (oam_debug, _reusable_oam) = gb_oam_snapshot(emu, req.show_oam_viewer, reusable_oam);
    let palette_debug = gb_palette_snapshot(emu, req.any_viewer_open, req);

    let graphics_data = if req.any_vram_viewer_open {
        let ppu = emu.ppu_registers();
        let cgb_mode = emu.is_cgb_mode();
        let src = emu.vram();
        let mut vram_buf = reusable_vram.unwrap_or_default();
        vram_buf.resize(src.len(), 0);
        vram_buf.copy_from_slice(src);
        Some(ConsoleGraphicsData::Gb(GbGraphicsData {
            vram: vram_buf,
            ppu,
            cgb_mode,
            bg_palette_ram: emu.ppu_bg_palette_ram_snapshot(),
            obj_palette_ram: emu.ppu_obj_palette_ram_snapshot(),
            color_correction: req.render.color_correction,
            color_correction_matrix: req.render.color_correction_matrix,
            dmg_palette_preset: req.render.dmg_palette_preset,
        }))
    } else {
        None
    };

    let disassembly_view = if req.show_disassembler {
        let pc_changed = req.last_disasm_pc != Some(emu.cpu_pc());
        if pc_changed {
            let mut breakpoints: Vec<u16> = emu.iter_breakpoints().collect();
            breakpoints.sort_unstable();
            Some(DisassemblyView {
                pc: emu.cpu_pc(),
                lines: disassemble_around(|addr| emu.peek_byte(addr), emu.cpu_pc(), 12, 26),
                breakpoints,
            })
        } else {
            None
        }
    } else {
        None
    };

    let rom_debug = if req.show_rom_info {
        Some(gb_rom_info(emu))
    } else {
        None
    };

    let memory_page = if req.show_memory_viewer {
        let mut buf = reusable_memory_page.unwrap_or_else(|| Vec::with_capacity(256));
        buf.clear();
        let start = req.memory_view_start;
        for i in 0..256u16 {
            let addr = start.wrapping_add(i);
            buf.push((addr, emu.peek_byte(addr)));
        }
        Some(buf)
    } else {
        reusable_memory_page.map(|mut v| {
            v.clear();
            v
        })
    };

    let memory_search_results = if let Some(ref search) = req.memory_search {
        let mut results = Vec::new();
        if !search.pattern.is_empty() {
            let pattern_len = search.pattern.len();
            for start_addr in 0..=(0x10000usize - pattern_len) {
                if results.len() >= search.max_results {
                    break;
                }
                let matched = (0..pattern_len)
                    .all(|j| emu.peek_byte_raw((start_addr + j) as u16) == search.pattern[j]);
                if matched {
                    let matched_bytes: Vec<u8> = (0..pattern_len)
                        .map(|j| emu.peek_byte_raw((start_addr + j) as u16))
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

    let rom_bytes = emu.cartridge_rom_bytes();
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

    let perf_info = gb_info.as_ref().map(|di| PerfInfo {
        fps: di.fps,
        speed_mode_label: di.speed_mode_label.to_string(),
        frames_in_flight: di.frames_in_flight,
        cycles: di.cycles,
        platform_name: "Game Boy",
        hardware_label: format!("{:?}", di.hardware_mode),
        hardware_pref_label: format!("{:?}", di.hardware_mode_preference),
    });

    UiFrameData {
        cpu_debug,
        perf_info,
        apu_debug,
        oam_debug,
        palette_debug,
        rom_debug,
        input_debug,
        graphics_data,
        disassembly_view,
        memory_page,
        memory_search_results,
        rom_page,
        rom_size,
        rom_search_results,
    }
}
