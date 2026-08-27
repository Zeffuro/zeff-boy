use super::collect_coleco_snapshot;
use crate::debug::ConsoleGraphicsData;
use crate::emu_thread::{RenderSettings, ReusableBuffers, SnapshotRequest};
use crate::settings::{ColorCorrection, DmgPalettePreset, NesPaletteMode};

fn snapshot_request() -> SnapshotRequest {
    SnapshotRequest {
        want_debug_info: true,
        want_perf_info: true,
        any_viewer_open: true,
        any_vram_viewer_open: true,
        show_oam_viewer: true,
        show_apu_viewer: true,
        show_disassembler: true,
        show_rom_info: true,
        show_memory_viewer: true,
        memory_view_start: 0,
        show_rom_viewer: true,
        show_instruction_trace: false,
        trace_after_sequence: None,
        rom_view_start: 0,
        last_disasm_pc: None,
        last_disasm_mapping: None,
        disasm_target: None,
        memory_search: None,
        rom_search: None,
        render: RenderSettings {
            color_correction: ColorCorrection::None,
            color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            dmg_palette_preset: DmgPalettePreset::default(),
            nes_palette_mode: NesPaletteMode::default(),
            nes_custom_palette: None,
            pce_overscan_mode: crate::settings::PceOverscanMode::default(),
            pce_palette_mode: crate::settings::PcePaletteMode::default(),
            sgb_border_enabled: false,
        },
    }
}

fn buffers() -> ReusableBuffers {
    ReusableBuffers {
        audio: None,
        vram: None,
        oam: None,
        memory_page: None,
        nes_chr: None,
        nes_nametable: None,
    }
}

#[test]
fn snapshot_exposes_coleco_debug_and_tms_data() {
    let mut bios = [0; 8 * 1024];
    bios[..5].copy_from_slice(&[0x3E, 0x90, 0xD3, 0xE0, 0x76]);
    let mut cartridge = vec![0; 8 * 1024];
    cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
    let mut emu = zeff_coleco_core::Emulator::new(&cartridge, &bios, 48_000).unwrap();
    emu.set_opcode_log_enabled(true);
    let _ = emu.step_instruction();
    let _ = emu.step_instruction();

    let data = collect_coleco_snapshot(&emu, &snapshot_request(), buffers());

    assert!(data.cpu_debug.is_some());
    assert!(data.perf_info.is_some());
    assert!(data.input_debug.is_some());
    assert!(data.apu_debug.is_some());
    assert!(data.oam_debug.is_some());
    assert!(data.palette_debug.is_some());
    assert!(data.rom_debug.is_some());
    assert!(data.memory_page.is_some());
    assert!(data.rom_page.is_some());
    assert!(data.disassembly_view.is_some());
    let ConsoleGraphicsData::Coleco(gfx) = data.graphics_data.unwrap() else {
        panic!("Coleco snapshot should include Coleco TMS data");
    };
    assert!(!gfx.mode4.enabled);
    assert_eq!(gfx.vram.len(), 0x4000);
    assert_eq!(gfx.oam.len(), 32 * 4);
}
