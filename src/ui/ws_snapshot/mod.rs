use crate::emu_thread::{ReusableBuffers, SnapshotRequest};
use zeff_emu_common::address::Address;
use zeff_ws_core::emulator::Emulator;

mod apu;
mod cpu;
mod input;
mod rom_info;

#[cfg(test)]
mod tests;

const WS_SEARCH_RANGES: &[(Address, Address)] = &[(0, 0x0F_FFFF)];

pub(crate) fn collect_ws_snapshot(
    emu: &Emulator,
    snapshot: &SnapshotRequest,
    buffers: ReusableBuffers,
) -> super::UiFrameData {
    let rom_bytes = emu.cartridge_rom_bytes();
    let mut data = super::UiFrameData {
        perf_info: snapshot
            .want_perf_info
            .then(|| rom_info::ws_perf_snapshot(emu)),
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
            buffers.memory_page,
            |addr| emu.cpu_peek8(addr),
        ),
        memory_search_results: super::build_memory_search_ranges(
            snapshot.memory_search.as_ref(),
            WS_SEARCH_RANGES,
            |addr| emu.cpu_peek8(addr),
        ),
        ..Default::default()
    };

    if snapshot.want_debug_info {
        data.cpu_debug = Some(cpu::ws_cpu_snapshot(emu));
        data.input_debug = Some(input::ws_input_snapshot(emu));
    }

    if snapshot.show_apu_viewer {
        data.apu_debug = Some(apu::ws_apu_snapshot(emu));
    }

    if snapshot.show_rom_info {
        data.rom_debug = Some(rom_info::ws_rom_info(emu));
    }

    data
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
