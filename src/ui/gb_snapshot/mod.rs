mod apu;
mod cpu;
mod input;
mod oam;
mod palette;
mod rom_info;

use super::UiFrameData;
use crate::debug::{ConsoleGraphicsData, GbGraphicsData, PerfInfo, disassemble_around};
use crate::emu_thread::SnapshotRequest;
use zeff_emu_common::address::{Address, narrow_u16};
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
    reusable_memory_page: Option<Vec<(Address, u8)>>,
) -> UiFrameData {
    let gb_info = if req.want_debug_info || req.want_perf_info {
        Some(emu.snapshot())
    } else {
        None
    };

    let cpu_debug = gb_info.as_ref().map(gb_cpu_snapshot);
    let input_debug = gb_info.as_ref().map(gb_input_snapshot);
    let apu_debug = gb_apu_snapshot(emu, req.show_apu_viewer);
    let (oam_debug, reusable_oam) = gb_oam_snapshot(emu, req.show_oam_viewer, reusable_oam);
    let palette_debug = gb_palette_snapshot(emu, req.any_viewer_open, req);

    let graphics_data = if req.any_vram_viewer_open {
        let ppu = emu.ppu_registers();
        let cgb_mode = emu.is_cgb_mode();
        let src = emu.vram();
        let mut vram_buf = reusable_vram.unwrap_or_default();
        vram_buf.resize(src.len(), 0);
        vram_buf.copy_from_slice(src);
        let mut oam_buf = reusable_oam.unwrap_or_default();
        let oam_src = emu.oam();
        oam_buf.resize(oam_src.len(), 0);
        oam_buf.copy_from_slice(oam_src);
        Some(ConsoleGraphicsData::Gb(GbGraphicsData {
            vram: vram_buf,
            oam: oam_buf,
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

    let disasm_target = req.disasm_target.and_then(|target| {
        target.storage_offset.and_then(|storage_offset| {
            u16::try_from(target.cpu_address)
                .ok()
                .filter(|address| *address < 0x8000)
                .map(|address| (target, address, storage_offset))
        })
    });
    let current_disasm = disasm_target.map_or_else(
        || (emu.cpu_pc().into(), Some(emu.rom_mapping_token())),
        |(_, address, storage_offset)| (Address::from(address), Some(storage_offset)),
    );
    let disassembly_view = super::build_disassembly_view(
        req.show_disassembler,
        req.last_disasm_pc.map(|pc| (pc, req.last_disasm_mapping)),
        current_disasm,
        || {
            if let Some((target, address, _)) = disasm_target {
                let rom = emu.cartridge_rom_bytes();
                disassemble_around(
                    |addr| target_rom_byte(rom, target, address, addr),
                    address,
                    12,
                    26,
                )
            } else {
                disassemble_around(|addr| emu.peek_byte(addr), emu.cpu_pc(), 12, 26)
            }
        },
        emu.iter_breakpoints().map(Address::from),
        emu.iter_one_shot_breakpoints().map(Address::from),
    )
    .map(|mut view| {
        view.is_navigation_target = disasm_target.is_some();
        view.is_static_target = disasm_target.is_some();
        view.rom_breakpoints = emu
            .iter_rom_breakpoints()
            .filter_map(|offset| u64::try_from(offset).ok())
            .collect();
        view.hit_rom_breakpoint = emu
            .debug_hit_rom_breakpoint()
            .and_then(|offset| u64::try_from(offset).ok());
        for line in &mut view.lines {
            line.storage_offset = if let Some((target, target_address, _)) = disasm_target {
                u16::try_from(line.address)
                    .ok()
                    .and_then(|address| target_rom_offset(target, target_address, address))
            } else {
                u16::try_from(line.address)
                    .ok()
                    .and_then(|address| emu.rom_offset_for_cpu_address(address))
                    .map(|offset| offset as u64)
            };
            line.control_target_storage = line
                .control_target
                .and_then(|address| u16::try_from(address).ok())
                .and_then(|address| {
                    disasm_target
                        .and_then(|(target, target_address, _)| {
                            target_rom_offset(target, target_address, address)
                        })
                        .or_else(|| {
                            emu.rom_offset_for_cpu_address(address)
                                .map(|offset| offset as u64)
                        })
                });
        }
        view
    });

    let rom_debug = if req.show_rom_info {
        Some(gb_rom_info(emu))
    } else {
        None
    };

    let memory_page = super::build_memory_page(
        req.show_memory_viewer,
        req.memory_view_start,
        reusable_memory_page,
        |addr| emu.peek_byte(narrow_u16(addr)),
    );

    let memory_search_results = super::build_memory_search(req.memory_search.as_ref(), |addr| {
        emu.peek_byte_raw(narrow_u16(addr))
    });

    let rom_bytes = emu.cartridge_rom_bytes();
    let rom_size = rom_bytes.len() as u32;

    let rom_page = super::build_rom_page(req.show_rom_viewer, req.rom_view_start, rom_bytes);

    let rom_search_results = super::build_rom_search(req.rom_search.as_ref(), rom_bytes);

    let perf_info = gb_info.as_ref().map(|di| PerfInfo {
        fps: di.fps,
        target_fps: zeff_emu_common::system::System::GameBoy.target_fps(),
        speed_mode_label: di.speed_mode_label,
        frames_in_flight: di.frames_in_flight,
        cycles: di.cycles,
        platform_name: "Game Boy",
        hardware_label: format!("{:?}", di.hardware_mode).into(),
        hardware_pref_label: format!("{:?}", di.hardware_mode_preference).into(),
    });

    UiFrameData {
        core_features: None,
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

fn target_rom_offset(
    target: crate::debug::DisassemblyTarget,
    target_address: u16,
    address: u16,
) -> Option<u64> {
    let target_window = target_address & 0xC000;
    if target_window > 0x4000 || address & 0xC000 != target_window {
        return None;
    }
    let base = target
        .storage_offset?
        .checked_sub(u64::from(target_address & 0x3FFF))?;
    base.checked_add(u64::from(address & 0x3FFF))
}

fn target_rom_byte(
    rom: &[u8],
    target: crate::debug::DisassemblyTarget,
    target_address: u16,
    address: u16,
) -> u8 {
    target_rom_offset(target, target_address, address)
        .and_then(|offset| usize::try_from(offset).ok())
        .and_then(|offset| rom.get(offset))
        .copied()
        .unwrap_or(0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_rom_mapping_stays_in_the_symbols_bank() {
        let target = crate::debug::DisassemblyTarget {
            cpu_address: 0x4560,
            storage_offset: Some(0x8560),
            thumb: None,
        };
        assert_eq!(target_rom_offset(target, 0x4560, 0x4000), Some(0x8000));
        assert_eq!(target_rom_offset(target, 0x4560, 0x7FFF), Some(0xBFFF));
        assert_eq!(target_rom_offset(target, 0x4560, 0x3FFF), None);
        assert_eq!(target_rom_offset(target, 0x4560, 0x8000), None);
    }
}
