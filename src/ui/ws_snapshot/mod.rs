use crate::debug::v30_disassemble_around;
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

    let disasm_target = snapshot.disasm_target.filter(|target| {
        target.storage_offset.is_some_and(|offset| {
            target.cpu_address <= 0x0F_FFFF && offset < rom_bytes.len() as u64
        })
    });
    let current_disasm = disasm_target.map_or_else(
        || (emu.cpu_pc(), Some(emu.rom_mapping_token())),
        |target| (target.cpu_address, target.storage_offset),
    );
    data.disassembly_view = super::build_disassembly_view(
        snapshot.show_disassembler,
        snapshot
            .last_disasm_pc
            .map(|pc| (pc, snapshot.last_disasm_mapping)),
        current_disasm,
        || {
            v30_disassemble_around(
                |addr| {
                    disasm_target.map_or_else(
                        || emu.cpu_peek8(addr),
                        |target| target_rom_byte(rom_bytes, target, addr),
                    )
                },
                current_disasm.0,
                12,
                26,
            )
        },
        emu.iter_breakpoints(),
        emu.iter_one_shot_breakpoints(),
    )
    .map(|mut view| {
        view.is_navigation_target = disasm_target.is_some();
        view.is_static_target = disasm_target.is_some();
        for line in &mut view.lines {
            line.storage_offset = disasm_target
                .and_then(|target| target_rom_offset(target, line.address, rom_bytes.len()))
                .or_else(|| {
                    emu.rom_offset_for_cpu_address(line.address)
                        .map(|offset| offset as u64)
                });
            line.control_target_storage = line.control_target.and_then(|address| {
                disasm_target
                    .and_then(|target| target_rom_offset(target, address, rom_bytes.len()))
                    .or_else(|| {
                        emu.rom_offset_for_cpu_address(address)
                            .map(|offset| offset as u64)
                    })
            });
        }
        view
    });

    if snapshot.show_apu_viewer {
        data.apu_debug = Some(apu::ws_apu_snapshot(emu));
    }

    if snapshot.show_rom_info {
        data.rom_debug = Some(rom_info::ws_rom_info(emu));
    }

    data
}

fn target_rom_offset(
    target: crate::debug::DisassemblyTarget,
    address: Address,
    rom_len: usize,
) -> Option<u64> {
    let base = target.storage_offset?;
    let delta = address.wrapping_sub(target.cpu_address) & 0x0F_FFFF;
    let offset = base.checked_add(u64::from(delta))?;
    (offset < rom_len as u64).then_some(offset)
}

fn target_rom_byte(rom: &[u8], target: crate::debug::DisassemblyTarget, address: Address) -> u8 {
    target_rom_offset(target, address, rom.len())
        .and_then(|offset| usize::try_from(offset).ok())
        .and_then(|offset| rom.get(offset))
        .copied()
        .unwrap_or(0xFF)
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
