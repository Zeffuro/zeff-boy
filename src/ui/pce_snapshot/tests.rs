use std::path::PathBuf;

use crate::emu_backend::pce_profiles::LEMMINGS_JAPAN_CANONICAL_DISC_SHA256;
use crate::emu_core_trait::DebuggableEmulator;
use crate::emu_thread::{RenderSettings, ReusableBuffers};
use crate::settings::{ColorCorrection, DmgPalettePreset, NesPaletteMode};
use zeff_emu_common::debug::TraceExecMode;
use zeff_emu_common::time::FrameLifecycle;
use zeff_pce_core::hardware::{
    CdDisc, CdTrack, CdTrackMode, PceConsoleWiring, PceHuCardBoard, SYSTEM_CARD_V1_V2_IMAGE_LEN,
};

use super::*;

fn field<'a>(info: &'a RomDebugInfo, section: &str, name: &str) -> &'a str {
    info.sections
        .iter()
        .find(|candidate| candidate.heading == section)
        .and_then(|section| section.fields.iter().find(|(field, _)| *field == name))
        .map(|(_, value)| value.as_str())
        .unwrap()
}

fn disassembly_request() -> SnapshotRequest {
    SnapshotRequest {
        debug_target: crate::debug::DebugTarget::Primary,
        debug_revision: 0,
        want_debug_info: false,
        want_perf_info: false,
        any_viewer_open: false,
        any_vram_viewer_open: false,
        show_oam_viewer: false,
        show_apu_viewer: false,
        show_disassembler: true,
        show_rom_info: false,
        show_memory_viewer: false,
        memory_view_start: 0,
        show_rom_viewer: false,
        show_instruction_trace: false,
        trace_cursor: Default::default(),
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

fn debugger_request() -> SnapshotRequest {
    let mut request = disassembly_request();
    request.want_debug_info = true;
    request.any_viewer_open = true;
    request.show_oam_viewer = true;
    request.show_apu_viewer = true;
    request.show_disassembler = false;
    request
}

#[test]
fn hucard_rom_info_reports_board_size_and_wiring() {
    let mut rom = vec![0xEA; 0x2000];
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    let backend = PceBackend::new(rom, PathBuf::from("game.pce")).unwrap();
    let info = pce_rom_info(&backend);

    assert_eq!(field(&info, "HuCard", "Board"), "Plain");
    assert_eq!(field(&info, "HuCard", "ROM Size"), "8192 bytes");
    assert_eq!(field(&info, "Machine", "Console Wiring"), "PcEngine");
    assert_eq!(field(&info, "Machine", "Controller"), "TwoButton");
    assert_eq!(field(&info, "Content", "ROM Path"), "game.pce");
}

#[test]
fn snapshot_populates_pce_performance_info() {
    let mut rom = vec![0xEA; 0x2000];
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    let backend = PceBackend::new(rom, PathBuf::from("game.pce")).unwrap();
    let mut request = disassembly_request();
    request.show_disassembler = false;
    request.want_perf_info = true;

    let perf = collect_pce_snapshot(&backend, &request, None)
        .perf_info
        .unwrap();
    assert_eq!(perf.platform_name, "PC Engine");
    assert!(perf.hardware_label.contains("Base"));
}

#[test]
fn cd_rom_info_reports_mixed_track_layout_and_system_card() {
    let disc = CdDisc::new(vec![
        CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, vec![0; 2048]).unwrap(),
        CdTrack::from_index1_data(2, 0, None, 1, CdTrackMode::Audio, vec![0; 2352]).unwrap(),
    ])
    .unwrap();
    let backend = PceBackend::new_cdrom2(
        vec![0xEA; SYSTEM_CARD_V1_V2_IMAGE_LEN],
        disc,
        crate::emu_backend::pce::PceCdBackendConfig {
            system_card_board: PceHuCardBoard::SystemCardV3,
            cue_path: PathBuf::from("set.7z/disc.cue"),
            source_path: PathBuf::from("set.7z"),
            content_hash: [0xAB; 32],
            content_crc32: 0xAB,
            source_disc_hash: LEMMINGS_JAPAN_CANONICAL_DISC_SHA256,
            console_wiring: PceConsoleWiring::TurboGrafx16,
            arcade_card_mode: zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
        },
    )
    .unwrap();
    let info = pce_rom_info(&backend);

    assert_eq!(field(&info, "Disc", "Tracks"), "2");
    assert_eq!(field(&info, "Disc", "Audio Tracks"), "1");
    assert!(field(&info, "Disc", "Layout").contains("02:Audio@1+1"));
    assert_eq!(field(&info, "Machine", "System Card"), "SystemCardV3");
    assert_eq!(field(&info, "Content", "Source Path"), "set.7z");
    assert_eq!(field(&info, "Canonical Title", "ID"), "pce-cd:jp:lemmings");
    assert_eq!(field(&info, "Canonical Title", "Title"), "Lemmings");
}

#[test]
fn snapshot_builds_huc6280_disassembly_with_rom_offsets() {
    let mut rom = vec![0xEA; 0x2000];
    rom[0] = 0x73;
    rom[1..7].copy_from_slice(&[0x34, 0x12, 0x78, 0x56, 0xBC, 0x9A]);
    rom[0x1FFE..].copy_from_slice(&0x0000_u16.to_le_bytes());
    let backend = PceBackend::new(rom, PathBuf::from("game.pce")).unwrap();
    let request = disassembly_request();

    let data = collect_pce_snapshot(&backend, &request, None);
    let view = data.disassembly_view.unwrap();
    let line = view
        .lines
        .iter()
        .find(|line| line.address == Address::from(0u16))
        .unwrap();
    assert_eq!(line.mnemonic.as_str(), "TII $1234,$5678,$9ABC");
    assert_eq!(line.storage_offset, Some(0));
    assert_eq!(line.bank, Some(0));
}

#[test]
fn snapshot_exposes_pce_execution_history() {
    let mut rom = vec![0xEA; 0x2000];
    rom[0] = 0xD4;
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    let mut backend = PceBackend::new(rom, PathBuf::from("history.pce")).unwrap();
    backend.set_opcode_history_enabled(true);
    backend.debug_suspend();
    backend.debug_step();
    backend.step_frame();

    let data = collect_pce_snapshot(&backend, &debugger_request(), None);
    let cpu = data.cpu_debug.unwrap();
    assert_eq!(cpu.cpu_state, "Suspended");
    assert_eq!(cpu.recent_opcodes[0].address, 0xE000);
    assert_eq!(cpu.recent_opcodes[0].bytes, [0xD4]);
    assert!(cpu.last_opcode_line.contains("E000"));
}

#[test]
fn snapshot_exposes_logical_breakpoint_and_watchpoint_controls() {
    let mut rom = vec![0xEA; 0x2000];
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    let mut backend = PceBackend::new(rom, PathBuf::from("debug.pce")).unwrap();
    DebuggableEmulator::add_breakpoint(&mut backend, 0xE000);
    DebuggableEmulator::add_one_shot_breakpoint(&mut backend, 0xE010);
    DebuggableEmulator::add_breakpoint_after(&mut backend, 0xE020, 4);
    DebuggableEmulator::add_watchpoint_range(
        &mut backend,
        0x2000,
        0x200F,
        zeff_emu_common::debug::WatchType::ReadWrite,
    );
    DebuggableEmulator::set_event_breakpoint(&mut backend, DebugEvent::Interrupt, true);
    DebuggableEmulator::set_event_breakpoint(&mut backend, DebugEvent::Dma, true);

    backend.step_frame();
    let data = collect_pce_snapshot(&backend, &debugger_request(), None);
    let cpu = data.cpu_debug.unwrap();

    assert_eq!(cpu.breakpoints, [0xE000, 0xE010, 0xE020]);
    assert_eq!(cpu.one_shot_breakpoints, [0xE010]);
    assert_eq!(cpu.breakpoint_hit_conditions[0].address, 0xE020);
    assert_eq!(cpu.breakpoint_hit_conditions[0].target_hits, 4);
    assert_eq!(cpu.watchpoints[0].address, 0x2000);
    assert_eq!(cpu.watchpoints[0].end_address, 0x200F);
    assert_eq!(cpu.hit_breakpoint, Some(0xE000));
    assert_eq!(
        cpu.supported_events,
        [DebugEvent::Interrupt, DebugEvent::Dma]
    );
    assert_eq!(
        cpu.event_breakpoints,
        [DebugEvent::Interrupt, DebugEvent::Dma]
    );
}

#[test]
fn backend_snapshot_collects_page_qualified_huc6280_trace() {
    let mut rom = vec![0xEA; 0x2000];
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    let mut backend = PceBackend::new(rom, PathBuf::from("trace.pce")).unwrap();
    DebuggableEmulator::set_instruction_trace_enabled(&mut backend, true);
    backend.debug_suspend();
    backend.debug_step();
    backend.step_frame();
    let mut backend = crate::emu_backend::EmuBackend::from_pce(backend);
    let mut request = disassembly_request();
    request.show_instruction_trace = true;

    let data = crate::ui::collect_backend_snapshot(
        &mut backend,
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

    let trace = data.instruction_trace.unwrap();
    assert!(trace.enabled);
    assert_eq!(trace.retained, 1);
    assert_eq!(trace.entries[0].mode, TraceExecMode::HuC6280);
    assert_eq!(trace.entries[0].pc, 0xE000);
    assert_eq!(trace.entries[0].bank, Some(0));
    assert_eq!(trace.entries[0].physical_rom_offset, Some(0));
    assert_eq!(trace.entries[0].instruction_bytes(), &[0xEA]);
}

#[test]
fn snapshot_emits_base_vdc_graphics_when_a_vram_viewer_is_open() {
    let mut rom = vec![0xEA; 0x2000];
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    let backend = PceBackend::new(rom, PathBuf::from("graphics.pce")).unwrap();
    let mut request = debugger_request();
    request.any_vram_viewer_open = true;

    let data = collect_pce_snapshot(&backend, &request, None);
    let Some(ConsoleGraphicsData::Pce(graphics)) = data.graphics_data else {
        panic!("PCE graphics snapshot missing");
    };
    assert_eq!(
        graphics.vdc1.vram.len(),
        zeff_pce_core::hardware::VDC_VRAM_WORDS
    );
    assert!(graphics.vdc2.is_none());
    assert_eq!(
        graphics.palette.len(),
        zeff_pce_core::hardware::VCE_PALETTE_COLORS
    );
}

#[test]
fn graphics_snapshot_keeps_supergrafx_vdc2_separate() {
    let snapshot = PceGraphicsSnapshot {
        vdc1: crate::emu_backend::pce::PceVdcGraphicsSnapshot {
            vram: vec![0x1111],
            registers: [1; 0x14],
        },
        vdc2: Some(crate::emu_backend::pce::PceVdcGraphicsSnapshot {
            vram: vec![0x2222],
            registers: [2; 0x14],
        }),
        palette: [zeff_pce_core::hardware::VceColor::new(0); 512],
    };

    let ConsoleGraphicsData::Pce(graphics) = pce_graphics_snapshot(snapshot) else {
        unreachable!();
    };
    assert_eq!(graphics.vdc1.vram, [0x1111]);
    assert_eq!(graphics.vdc2.unwrap().vram, [0x2222]);
}

#[test]
fn static_disassembly_target_reads_the_selected_physical_page() {
    let mut rom = vec![0xEA; 0x4000];
    rom[0x2000..0x2002].copy_from_slice(&[0xA9, 0x42]);
    rom[0x1FFE..0x2000].copy_from_slice(&0x0000_u16.to_le_bytes());
    let backend = PceBackend::new(rom, PathBuf::from("game.pce")).unwrap();
    let mut request = disassembly_request();
    request.disasm_target = Some(crate::debug::DisassemblyTarget::Cpu(
        crate::debug::CpuDisassemblyTarget {
            cpu_address: 0xA000,
            storage_offset: None,
            bank: Some(1),
            thumb: None,
        },
    ));

    let data = collect_pce_snapshot(&backend, &request, None);
    let view = data.disassembly_view.unwrap();
    let line = view
        .lines
        .iter()
        .find(|line| line.address == Address::from(0xA000u16))
        .unwrap();
    assert_eq!(line.mnemonic.as_str(), "LDA #$42");
    assert_eq!(line.bank, Some(1));
    assert!(view.is_static_target);
}

#[test]
fn snapshot_populates_pce_hardware_debug_windows() {
    let mut rom = vec![0xEA; 0x2000];
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    let mut backend = PceBackend::new(rom, PathBuf::from("hardware.pce")).unwrap();
    backend.set_apu_debug_capture_enabled(true);
    backend.step_frame();

    let data = collect_pce_snapshot(&backend, &debugger_request(), None);
    let cpu = data.cpu_debug.unwrap();
    assert!(cpu.sections.iter().any(|section| section.heading == "VDC1"));
    assert!(cpu.sections.iter().any(|section| section.heading == "VCE"));

    let apu = data.apu_debug.unwrap();
    assert_eq!(apu.channels.len(), 6);
    assert!(!apu.master_waveform.is_empty());
    assert!(
        apu.channels
            .iter()
            .all(|channel| channel.waveform.len() == apu.master_waveform.len())
    );
    assert_eq!(apu.extra_sections[0].heading, "PSG Wave RAM");
    assert_eq!(apu.extra_sections[0].lines.len(), 6);

    let input = data.input_debug.unwrap();
    assert_eq!(input.sections[0].heading, "Controller Port");
    assert_eq!(input.sections[1].heading, "Two-button Pad");

    let palette = data.palette_debug.unwrap();
    assert_eq!(palette.groups.len(), 2);
    assert!(palette.groups.iter().all(|group| group.rows.len() == 16));

    let oam = data.oam_debug.unwrap();
    assert_eq!(oam.rows.len(), 64);
    assert_eq!(oam.rows[0][0], "1");
}

#[test]
fn cd_snapshot_exposes_transport_cdda_and_adpcm_state() {
    let disc = CdDisc::new(vec![
        CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, vec![0; 2048]).unwrap(),
    ])
    .unwrap();
    let backend = PceBackend::new_cdrom2(
        vec![0xEA; SYSTEM_CARD_V1_V2_IMAGE_LEN],
        disc,
        crate::emu_backend::pce::PceCdBackendConfig {
            system_card_board: PceHuCardBoard::SystemCardV1V2,
            cue_path: PathBuf::from("disc.cue"),
            source_path: PathBuf::from("disc.cue"),
            content_hash: [0xCD; 32],
            content_crc32: 0xCD,
            source_disc_hash: [0xCD; 32],
            console_wiring: PceConsoleWiring::PcEngine,
            arcade_card_mode: zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
        },
    )
    .unwrap();

    let data = collect_pce_snapshot(&backend, &debugger_request(), None);
    let cpu = data.cpu_debug.unwrap();
    for heading in ["CD/SCSI", "CD IRQ", "CDDA", "ADPCM"] {
        assert!(
            cpu.sections
                .iter()
                .any(|section| section.heading == heading)
        );
    }
    let apu = data.apu_debug.unwrap();
    assert!(
        apu.extra_sections
            .iter()
            .any(|section| section.heading == "CDDA")
    );
    assert!(
        apu.extra_sections
            .iter()
            .any(|section| section.heading == "ADPCM")
    );
}
