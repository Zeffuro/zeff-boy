mod apu;
mod cpu;
mod graphics;
mod oam;
mod palette;
mod rom_info;

use crate::debug::nes_disassemble_around;
use crate::emu_thread::SnapshotRequest;

use apu::nes_apu_snapshot;
use cpu::nes_cpu_snapshot;
use graphics::{nes_disasm_peek_byte, nes_graphics_snapshot};
use oam::nes_oam_snapshot;
use palette::nes_palette_snapshot;
use rom_info::nes_rom_info;

pub(crate) fn collect_nes_snapshot(
    emu: &mut zeff_nes_core::emulator::Emulator,
    snapshot: &SnapshotRequest,
    reusable_chr: Option<Vec<u8>>,
    reusable_nametable: Option<Vec<u8>>,
    reusable_memory_page: Option<Vec<(u16, u8)>>,
) -> super::UiFrameData {
    let mut data = super::UiFrameData::default();

    emu.set_opcode_log_enabled(snapshot.want_debug_info);

    if snapshot.want_perf_info {
        data.perf_info = Some(crate::debug::PerfInfo {
            fps: 0.0,
            speed_mode_label: "1×",
            frames_in_flight: 0,
            cycles: emu.cpu_cycles(),
            platform_name: "NES",
            hardware_label: emu.cartridge_header().mapper_label().into(),
            hardware_pref_label: format!("{:?}", emu.cartridge_header().timing).into(),
        });
    }

    if snapshot.want_debug_info {
        data.cpu_debug = Some(nes_cpu_snapshot(emu));
    }

    if snapshot.show_apu_viewer {
        data.apu_debug = nes_apu_snapshot(emu, true);
    }

    data.disassembly_view = super::build_disassembly_view(
        snapshot.show_disassembler,
        snapshot.last_disasm_pc,
        emu.cpu_pc(),
        || {
            nes_disassemble_around(
                |addr| nes_disasm_peek_byte(emu.bus(), addr),
                emu.cpu_pc(),
                12,
                26,
            )
        },
        emu.iter_breakpoints(),
    );

    if snapshot.show_rom_info {
        data.rom_debug = Some(nes_rom_info(emu));
    }

    if snapshot.any_vram_viewer_open {
        data.graphics_data = Some(nes_graphics_snapshot(emu, reusable_chr, reusable_nametable));
    }

    if snapshot.show_oam_viewer {
        data.oam_debug = Some(nes_oam_snapshot(emu));
    }

    if snapshot.any_viewer_open {
        data.palette_debug = Some(nes_palette_snapshot(emu));
    }

    data.memory_page = super::build_memory_page(
        snapshot.show_memory_viewer,
        snapshot.memory_view_start,
        reusable_memory_page,
        |addr| emu.cpu_peek(addr),
    );

    if snapshot.show_rom_viewer {
        let rom_header = emu.cartridge_header();
        let prg_size = rom_header.prg_rom_size;
        data.rom_size = prg_size as u32;
        let start = snapshot.rom_view_start as usize;
        let mut page = Vec::with_capacity(256);
        for i in 0..256usize {
            let offset = start + i;
            if offset < prg_size {
                let addr = 0x8000u16.wrapping_add(offset as u16);
                page.push((offset as u32, emu.cpu_peek(addr)));
            }
        }
        data.rom_page = Some(page);
    }

    data
}
