mod apu;
mod cpu;
mod graphics;
mod oam;
mod palette;
mod rom_info;

use crate::emu_thread::SnapshotRequest;

use apu::nes_apu_snapshot;
use cpu::nes_cpu_snapshot;
use graphics::{nes_disassembly_view, nes_graphics_snapshot};
use oam::nes_oam_snapshot;
use palette::nes_palette_snapshot;
use rom_info::nes_rom_info;

pub(crate) fn collect_nes_snapshot(
    emu: &mut zeff_nes_core::emulator::Emulator,
    snapshot: &SnapshotRequest,
) -> super::UiFrameData {
    let mut data = super::empty_frame_data();

    emu.set_opcode_log_enabled(snapshot.want_debug_info);

    if snapshot.want_perf_info {
        data.perf_info = Some(crate::debug::PerfInfo {
            fps: 0.0,
            speed_mode_label: "1×".to_string(),
            frames_in_flight: 0,
            cycles: emu.cpu_cycles(),
            platform_name: "NES",
            hardware_label: emu.cartridge_header().mapper_label(),
            hardware_pref_label: format!("{:?}", emu.cartridge_header().timing),
        });
    }

    if snapshot.want_debug_info {
        data.cpu_debug = Some(nes_cpu_snapshot(emu));
    }

    if snapshot.show_apu_viewer {
        data.apu_debug = nes_apu_snapshot(emu, true);
    }

    if snapshot.show_disassembler {
        let pc_changed = snapshot.last_disasm_pc != Some(emu.cpu_pc());
        if pc_changed {
            data.disassembly_view = Some(nes_disassembly_view(emu));
        }
    }

    if snapshot.show_rom_info {
        data.rom_debug = Some(nes_rom_info(emu));
    }

    if snapshot.any_vram_viewer_open {
        data.graphics_data = Some(nes_graphics_snapshot(emu));
    }

    if snapshot.show_oam_viewer {
        data.oam_debug = Some(nes_oam_snapshot(emu));
    }

    if snapshot.any_viewer_open {
        data.palette_debug = Some(nes_palette_snapshot(emu));
    }

    if snapshot.show_memory_viewer {
        let start = snapshot.memory_view_start;
        let mut page = Vec::with_capacity(256);
        for i in 0..256u16 {
            let addr = start.wrapping_add(i);
            page.push((addr, emu.cpu_peek(addr)));
        }
        data.memory_page = Some(page);
    }

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

