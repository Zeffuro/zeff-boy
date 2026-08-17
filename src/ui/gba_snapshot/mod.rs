use crate::debug::gba_disassemble_around;
use crate::emu_thread::{ReusableBuffers, SnapshotRequest};

mod apu;
mod cpu;
mod graphics;
mod perf;
mod rom_info;

#[cfg(test)]
mod tests;

const GBA_BASE_SEARCH_RANGES: &[(u32, u32)] = &[
    (0x0200_0000, 0x0203_FFFF),
    (0x0300_0000, 0x0300_7FFF),
    (0x0400_0000, 0x0400_03FF),
    (0x0500_0000, 0x0500_03FF),
    (0x0600_0000, 0x0601_7FFF),
    (0x0700_0000, 0x0700_03FF),
];

pub(crate) fn collect_gba_snapshot(
    emu: &zeff_gba_core::emulator::Emulator,
    snapshot: &SnapshotRequest,
    mut buffers: ReusableBuffers,
) -> super::UiFrameData {
    let rom_bytes = emu.cartridge_rom_bytes();
    let mut search_ranges = GBA_BASE_SEARCH_RANGES.to_vec();
    if !rom_bytes.is_empty() {
        let rom_end = 0x0800_0000u32.saturating_add(rom_bytes.len() as u32 - 1);
        search_ranges.push((0x0800_0000, rom_end.min(0x09FF_FFFF)));
    }
    if emu.has_battery() {
        search_ranges.push((0x0E00_0000, 0x0E00_FFFF));
    }

    let memory_page_buffer = buffers.memory_page.take();
    let mut data = super::UiFrameData {
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
            memory_page_buffer,
            |addr| emu.cpu_peek8(addr),
        ),
        memory_search_results: super::build_memory_search_ranges(
            snapshot.memory_search.as_ref(),
            &search_ranges,
            |addr| emu.cpu_peek8(addr),
        ),
        ..Default::default()
    };

    if snapshot.show_rom_info {
        data.rom_debug = Some(rom_info::gba_rom_info(emu));
    }

    if snapshot.want_debug_info {
        data.cpu_debug = Some(cpu::gba_cpu_snapshot(emu));
    }

    let disasm_target = snapshot.disasm_target.filter(|target| {
        (0x0800_0000..=0x0DFF_FFFF).contains(&target.cpu_address)
            && target.storage_offset < rom_bytes.len() as u64
    });
    let thumb = emu.cpu_thumb_state();
    let current_disasm = disasm_target.map_or_else(
        || (emu.cpu_pc(), Some(u64::from(thumb))),
        |target| (target.cpu_address, Some(target.storage_offset)),
    );
    data.disassembly_view = super::build_disassembly_view(
        snapshot.show_disassembler,
        snapshot
            .last_disasm_pc
            .map(|pc| (pc, snapshot.last_disasm_mapping)),
        current_disasm,
        || {
            gba_disassemble_around(
                |addr| {
                    disasm_target.map_or_else(
                        || emu.cpu_peek8(addr),
                        |target| target_rom_byte(rom_bytes, target, addr),
                    )
                },
                current_disasm.0,
                thumb,
                12,
                26,
            )
        },
        emu.iter_breakpoints(),
    )
    .map(|mut view| {
        view.is_navigation_target = disasm_target.is_some();
        view
    });

    if snapshot.any_vram_viewer_open {
        data.graphics_data = Some(crate::debug::ConsoleGraphicsData::Gba(
            graphics::gba_graphics_data(emu, buffers.vram.take()),
        ));
    }

    if snapshot.show_oam_viewer {
        data.oam_debug = Some(graphics::gba_oam_snapshot(emu));
    }

    if snapshot.any_viewer_open {
        data.palette_debug = Some(graphics::gba_palette_snapshot(emu));
    }

    if snapshot.show_apu_viewer {
        data.apu_debug = Some(apu::gba_apu_snapshot(emu));
    }

    if snapshot.want_perf_info {
        data.perf_info = Some(perf::gba_perf_info(emu));
    }

    data
}

fn target_rom_byte(rom: &[u8], target: crate::debug::DisassemblyTarget, address: u32) -> u8 {
    let offset = target
        .storage_offset
        .saturating_add(u64::from(address.wrapping_sub(target.cpu_address)));
    usize::try_from(offset)
        .ok()
        .and_then(|offset| rom.get(offset))
        .copied()
        .unwrap_or(0xFF)
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
