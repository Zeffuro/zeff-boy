use super::collect_sega8_snapshot;
use crate::debug::ConsoleGraphicsData;
use crate::emu_thread::{RenderSettings, ReusableBuffers, SnapshotRequest};
use crate::settings::{ColorCorrection, DmgPalettePreset, NesPaletteMode};
use zeff_sega8_core::emulator::Emulator;
use zeff_sega8_core::hardware::cartridge::SystemHint;

fn snapshot_request() -> SnapshotRequest {
    SnapshotRequest {
        want_debug_info: true,
        want_perf_info: true,
        any_viewer_open: true,
        any_vram_viewer_open: true,
        show_oam_viewer: true,
        show_apu_viewer: true,
        show_disassembler: false,
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
            sgb_border_enabled: false,
        },
    }
}

#[test]
fn sega8_snapshot_exposes_live_debug_and_graphics_data() {
    let mut emu = Emulator::new_with_hint(
        &[0x3E, 0x90, 0xD3, 0x7F, 0x76],
        48_000,
        SystemHint::MasterSystem,
    )
    .unwrap();
    emu.step_frame();

    let data = collect_sega8_snapshot(
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

    assert!(data.cpu_debug.is_some());
    assert!(data.perf_info.is_some());
    assert!(data.rom_debug.is_some());
    assert!(data.memory_page.is_some());
    assert!(data.rom_page.is_some());
    assert!(data.oam_debug.is_some());
    assert!(data.palette_debug.is_some());
    let apu = data
        .apu_debug
        .as_ref()
        .expect("Sega8 snapshot should include APU debug data");
    assert_eq!(apu.channels.len(), 4);
    assert!(apu.master_lines.iter().any(|line| line.contains("stereo=")));
    assert!(
        !apu.master_waveform.is_empty(),
        "Sega8 APU viewer should expose recent master samples"
    );
    assert!(
        !apu.channels[0].waveform.is_empty(),
        "Sega8 APU viewer should expose recent channel samples"
    );
    let ConsoleGraphicsData::Sega8(gfx) = data
        .graphics_data
        .as_ref()
        .expect("Sega8 snapshot should include graphics data")
    else {
        panic!("Sega8 snapshot should include Sega8 graphics data");
    };
    assert!(!gfx.mode4.enabled);
    assert_eq!(gfx.mode4.name_table_base, 0);
    assert_eq!(gfx.mode4.sprite_table_base, 0);
    let cpu = data
        .cpu_debug
        .as_ref()
        .expect("Sega8 snapshot should include CPU debug data");
    let vdp_section = cpu
        .sections
        .iter()
        .find(|section| section.heading == "VDP")
        .expect("Sega8 CPU debug should include a VDP section");
    assert!(vdp_section.lines.iter().any(|line| line.contains("mode4=")));
    assert_eq!(data.rom_size, 5);
}

#[test]
fn sega8_snapshot_exposes_z80_disassembly() {
    let mut emu = Emulator::new_with_hint(
        &[0x3E, 0x90, 0xD3, 0x7F, 0x76],
        48_000,
        SystemHint::MasterSystem,
    )
    .unwrap();
    emu.add_breakpoint(2);
    let mut request = snapshot_request();
    request.show_disassembler = true;

    let data = collect_sega8_snapshot(
        &emu,
        &request,
        ReusableBuffers {
            audio: None,
            vram: None,
            oam: None,
            memory_page: None,
            nes_chr: None,
            nes_nametable: None,
        },
    );
    let disassembly = data
        .disassembly_view
        .expect("Sega8 snapshot should include Z80 disassembly");

    assert_eq!(disassembly.pc, 0);
    assert!(disassembly.breakpoints.contains(&2));
    assert!(
        disassembly
            .lines
            .iter()
            .any(|line| line.mnemonic.as_str() == "LD A,$90")
    );
    assert!(
        disassembly
            .lines
            .iter()
            .any(|line| line.mnemonic.as_str() == "OUT ($7F),A")
    );
    assert!(
        disassembly
            .lines
            .iter()
            .any(|line| line.address == 0 && line.storage_offset == Some(0))
    );
}
