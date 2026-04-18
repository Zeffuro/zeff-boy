mod apu;
mod cpu;
mod input;
mod oam;
mod palette;
mod rom_info;

use super::UiFrameData;
use crate::debug::{
    ConsoleGraphicsData, GbGraphicsData, PerfInfo,
    disassemble_around,
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

    let disassembly_view = super::build_disassembly_view(
        req.show_disassembler,
        req.last_disasm_pc,
        emu.cpu_pc(),
        || disassemble_around(|addr| emu.peek_byte(addr), emu.cpu_pc(), 12, 26),
        emu.iter_breakpoints(),
    );

    let rom_debug = if req.show_rom_info {
        Some(gb_rom_info(emu))
    } else {
        None
    };

    let memory_page = super::build_memory_page(
        req.show_memory_viewer,
        req.memory_view_start,
        reusable_memory_page,
        |addr| emu.peek_byte(addr),
    );

    let memory_search_results =
        super::build_memory_search(req.memory_search.as_ref(), |addr| emu.peek_byte_raw(addr));

    let rom_bytes = emu.cartridge_rom_bytes();
    let rom_size = rom_bytes.len() as u32;

    let rom_page = super::build_rom_page(req.show_rom_viewer, req.rom_view_start, rom_bytes);

    let rom_search_results = super::build_rom_search(req.rom_search.as_ref(), rom_bytes);

    let perf_info = gb_info.as_ref().map(|di| PerfInfo {
        fps: di.fps,
        speed_mode_label: di.speed_mode_label,
        frames_in_flight: di.frames_in_flight,
        cycles: di.cycles,
        platform_name: "Game Boy",
        hardware_label: format!("{:?}", di.hardware_mode).into(),
        hardware_pref_label: format!("{:?}", di.hardware_mode_preference).into(),
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
