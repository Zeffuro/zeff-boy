use crate::debug::z80_disassemble_around;
use crate::emu_thread::{ReusableBuffers, SnapshotRequest};
use zeff_coleco_core::Emulator;
use zeff_emu_common::address::{Address, narrow_u16};

mod apu;
mod cpu;
mod graphics;
mod input;
mod rom_info;

#[cfg(test)]
mod tests;

const COLECO_ADDRESS_START: Address = 0x0000;
const COLECO_ADDRESS_END: Address = 0xFFFF;
const COLECO_SEARCH_RANGES: &[(Address, Address)] = &[(COLECO_ADDRESS_START, COLECO_ADDRESS_END)];

pub(crate) fn collect_coleco_snapshot(
    emu: &Emulator,
    snapshot: &SnapshotRequest,
    mut buffers: ReusableBuffers,
) -> super::UiFrameData {
    let rom_bytes = emu.bus().cartridge();
    let mut data = super::UiFrameData {
        perf_info: snapshot
            .want_perf_info
            .then(|| rom_info::coleco_perf_snapshot(emu)),
        rom_page: super::build_rom_page(
            snapshot.show_rom_viewer,
            snapshot.rom_view_start,
            rom_bytes,
        ),
        rom_size: rom_bytes.len() as u32,
        rom_search_results: super::build_rom_search(snapshot.rom_search.as_ref(), rom_bytes),
        memory_page: super::build_memory_page(
            snapshot.show_memory_viewer,
            snapshot.memory_view_start,
            buffers.memory_page.take(),
            |addr| emu.cpu_peek8(narrow_u16(addr)),
        ),
        memory_search_results: super::build_memory_search_ranges(
            snapshot.memory_search.as_ref(),
            COLECO_SEARCH_RANGES,
            |addr| emu.cpu_peek8(narrow_u16(addr)),
        ),
        ..Default::default()
    };

    if snapshot.want_debug_info {
        data.cpu_debug = Some(cpu::coleco_cpu_snapshot(emu));
        data.input_debug = Some(input::coleco_input_snapshot(emu));
    }

    data.disassembly_view = super::build_disassembly_view(
        snapshot.show_disassembler,
        snapshot
            .last_disasm_pc
            .map(|pc| (pc, snapshot.last_disasm_mapping)),
        (
            Address::from(emu.cpu().regs().pc),
            Some(emu.rom_mapping_token()),
        ),
        || z80_disassemble_around(|addr| emu.cpu_peek8(addr), emu.cpu().regs().pc, 12, 26),
        emu.iter_breakpoints(),
        emu.iter_one_shot_breakpoints(),
    )
    .map(|mut view| {
        for line in &mut view.lines {
            line.storage_offset = emu
                .rom_offset_for_cpu_address(narrow_u16(line.address))
                .map(|offset| offset as u64);
            line.control_target_storage = line
                .control_target
                .and_then(|address| emu.rom_offset_for_cpu_address(narrow_u16(address)))
                .map(|offset| offset as u64);
        }
        view
    });

    if snapshot.show_rom_info {
        data.rom_debug = Some(rom_info::coleco_rom_info(emu));
    }

    if snapshot.any_vram_viewer_open {
        data.graphics_data = Some(graphics::coleco_graphics_snapshot(
            emu,
            buffers.vram.take(),
            buffers.oam.take(),
        ));
    }

    if snapshot.show_oam_viewer {
        data.oam_debug = Some(graphics::coleco_oam_snapshot(emu));
    }

    if snapshot.show_apu_viewer {
        data.apu_debug = Some(apu::coleco_apu_snapshot(emu));
    }

    if snapshot.any_viewer_open {
        data.palette_debug = Some(graphics::coleco_palette_snapshot());
    }

    data
}
