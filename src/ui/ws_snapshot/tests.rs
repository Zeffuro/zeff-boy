use super::*;
use crate::emu_thread::RenderSettings;
use crate::settings::{ColorCorrection, DmgPalettePreset, NesPaletteMode};
use zeff_ws_core::hardware::cartridge::compute_footer_checksum;

fn minimal_ws_rom() -> Vec<u8> {
    let mut rom = vec![0xFF; 0x10000];
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    rom[0] = 0xF4;
    let footer = rom.len() - 10;
    rom[footer] = 0x01;
    rom[footer + 1] = 0x00;
    rom[footer + 2] = 0x23;
    rom[footer + 4] = 0x01;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn snapshot_request() -> SnapshotRequest {
    SnapshotRequest {
        want_debug_info: true,
        want_perf_info: true,
        any_viewer_open: false,
        any_vram_viewer_open: false,
        show_oam_viewer: false,
        show_apu_viewer: false,
        show_disassembler: false,
        show_rom_info: true,
        show_memory_viewer: true,
        memory_view_start: 0,
        show_rom_viewer: true,
        rom_view_start: 0,
        last_disasm_pc: None,
        memory_search: None,
        rom_search: None,
        render: RenderSettings {
            color_correction: ColorCorrection::None,
            color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            dmg_palette_preset: DmgPalettePreset::default(),
            nes_palette_mode: NesPaletteMode::default(),
            nes_custom_palette: None,
            sgb_border_enabled: false,
        },
    }
}

fn apu_snapshot_request() -> SnapshotRequest {
    SnapshotRequest {
        show_apu_viewer: true,
        ..snapshot_request()
    }
}

#[test]
fn wonder_swan_snapshot_exposes_data_for_app_rendering() {
    let rom = minimal_ws_rom();
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.add_breakpoint(0xF0000);
    emu.add_watchpoint(0x0000, zeff_emu_common::debug::WatchType::Write);
    emu.cpu_write8(0x0000, 0x5A);
    let data = collect_ws_snapshot(
        &emu,
        &snapshot_request(),
        ReusableBuffers {
            audio: None,
            vram: None,
            oam: None,
            memory_page: None,
            nes_chr: None,
            nes_nametable: None,
        },
    );

    assert!(data.perf_info.is_some());
    let cpu = data.cpu_debug.expect("WS CPU debug should be populated");
    assert_eq!(cpu.breakpoints, vec![0xF0000]);
    assert_eq!(cpu.watchpoints.len(), 1);
    assert_eq!(
        cpu.hit_watchpoint
            .as_ref()
            .map(|hit| (hit.address, hit.new_value)),
        Some((0x0000, 0x5A))
    );
    assert!(data.rom_debug.is_some());
    assert!(data.memory_page.is_some());
    assert!(data.rom_page.is_some());
    assert_eq!(data.rom_size, rom.len() as u32);
}

#[test]
fn wonder_swan_snapshot_exposes_apu_debug_when_viewer_is_open() {
    let rom = minimal_ws_rom();
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.cpu_write8(0x0000, 0x10);
    emu.io_write8(0x0080, 0x00);
    emu.io_write8(0x0081, 0x07);
    emu.io_write8(0x0088, 0xF8);
    emu.io_write8(0x0090, 0x01);

    let data = collect_ws_snapshot(
        &emu,
        &apu_snapshot_request(),
        ReusableBuffers {
            audio: None,
            vram: None,
            oam: None,
            memory_page: None,
            nes_chr: None,
            nes_nametable: None,
        },
    );

    let apu = data.apu_debug.expect("WS APU debug should be populated");
    assert_eq!(apu.channels.len(), 4);
    assert!(apu.channels[0].enabled);
    assert_eq!(apu.channels[0].waveform.len(), 32);
}
