use super::collect_gba_snapshot;
use crate::debug::ConsoleGraphicsData;
use crate::emu_thread::{RenderSettings, ReusableBuffers, SnapshotRequest};
use crate::settings::{ColorCorrection, DmgPalettePreset, NesPaletteMode};

fn minimal_gba_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom
}

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
            pce_overscan_mode: crate::settings::PceOverscanMode::default(),
            pce_palette_mode: crate::settings::PcePaletteMode::default(),
            sgb_border_enabled: false,
        },
    }
}

#[test]
fn gba_snapshot_exposes_live_debug_graphics_apu_and_perf_data() {
    let rom = minimal_gba_rom();
    let emu = zeff_gba_core::emulator::Emulator::new(&rom, 48_000)
        .expect("GBA emulator should initialize");

    let data = collect_gba_snapshot(
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
        .expect("GBA snapshot should include APU debug data");
    assert_eq!(apu.channels.len(), 6);
    assert_eq!(apu.master_waveform.len(), 512);
    assert_eq!(apu.channels[0].waveform.len(), 512);
    assert_eq!(apu.channels[4].waveform.len(), 512);
    assert!(
        apu.master_lines
            .iter()
            .any(|line| line.contains("SOUNDCNT_L="))
    );
    assert!(
        apu.extra_sections
            .iter()
            .any(|section| section.heading == "PSG Wave RAM")
    );
    assert_eq!(data.rom_size, rom.len() as u32);

    let ConsoleGraphicsData::Gba(gfx) = data
        .graphics_data
        .as_ref()
        .expect("GBA snapshot should include graphics data")
    else {
        panic!("GBA snapshot should include GBA graphics data");
    };
    assert_eq!(
        gfx.vram.len(),
        zeff_gba_core::hardware::constants::VRAM_SIZE
    );
    assert_eq!(
        gfx.palette_ram.len(),
        zeff_gba_core::hardware::constants::PALETTE_RAM_SIZE
    );
    assert_eq!(gfx.oam.len(), zeff_gba_core::hardware::constants::OAM_SIZE);
}

#[test]
fn gba_snapshot_includes_arm_disassembly_when_requested() {
    let rom = minimal_gba_rom();
    let emu = zeff_gba_core::emulator::Emulator::new(&rom, 48_000).unwrap();
    let mut request = snapshot_request();
    request.show_disassembler = true;

    let data = collect_gba_snapshot(
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

    assert!(
        data.disassembly_view
            .as_ref()
            .is_some_and(|view| !view.lines.is_empty())
    );
}

#[test]
fn gba_symbol_navigation_uses_the_symbol_instruction_set() {
    let mut rom = minimal_gba_rom();
    rom[0] = 0x00;
    rom[1] = 0xE0;
    let emu = zeff_gba_core::emulator::Emulator::new(&rom, 48_000).unwrap();
    let mut request = snapshot_request();
    request.show_disassembler = true;
    request.disasm_target = Some(crate::debug::DisassemblyTarget {
        cpu_address: 0x0800_0000,
        storage_offset: Some(0),
        bank: None,
        thumb: Some(true),
    });

    let data = collect_gba_snapshot(
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

    assert_eq!(
        data.disassembly_view.unwrap().lines[12].mnemonic.as_str(),
        "B $08000004"
    );
}
