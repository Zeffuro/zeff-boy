use std::path::PathBuf;

use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_thread::{EmuThread, RenderSettings, ReusableBuffers, SnapshotRequest};
use crate::settings::{ColorCorrection, DmgPalettePreset, NesPaletteMode};
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind};
use zeff_emu_common::save_ram::SaveRamKind;

pub(super) fn build_gb_test_rom() -> Vec<u8> {
    vec![0u8; 0x8000]
}

pub(super) fn build_gb_mbc3_rtc_test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x134..0x141].copy_from_slice(b"RTC HASH TEST");
    rom[0x147] = 0x10;
    rom[0x148] = 0x00;
    rom[0x149] = 0x03;
    rom
}

pub(super) fn build_nes_test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;

    let prg = 16;
    rom[prg] = 0xA9;
    rom[prg + 1] = 0x42;
    rom[prg + 2] = 0x85;
    rom[prg + 3] = 0x00;
    rom[prg + 4] = 0xEA;
    rom[prg + 5] = 0xEA;

    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom
}

pub(super) fn build_gba_test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom
}

pub(super) fn build_ws_test_rom() -> Vec<u8> {
    let mut rom = vec![0xFF; 0x10000];
    rom[0] = 0xF4;
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer + 1] = 0x00;
    rom[footer + 4] = 0x01;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

pub(super) fn build_sms_test_rom() -> Vec<u8> {
    vec![0x76]
}

pub(super) fn build_gb_backend() -> EmuBackend {
    let rom = build_gb_test_rom();
    let gb = zeff_gb_core::emulator::Emulator::from_rom_data(
        &rom,
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
    )
    .expect("GB emulator should initialize");
    EmuBackend::from_gb(gb, PathBuf::from("test.gb"))
}

pub(super) fn build_nes_backend() -> EmuBackend {
    let rom = build_nes_test_rom();
    let nes = zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0)
        .expect("NES emulator should initialize");
    EmuBackend::from_nes(nes, PathBuf::from("test.nes"))
}

pub(super) fn build_gba_backend() -> EmuBackend {
    let rom = build_gba_test_rom();
    let gba = zeff_gba_core::emulator::Emulator::new(&rom, 44_100)
        .expect("GBA emulator should initialize");
    EmuBackend::from_gba(gba, PathBuf::from("test.gba"))
}

pub(super) fn build_ws_backend() -> EmuBackend {
    let rom = build_ws_test_rom();
    let ws = zeff_ws_core::emulator::Emulator::new(&rom, 44_100)
        .expect("WonderSwan emulator should initialize");
    EmuBackend::from_ws(ws, PathBuf::from("test.ws"))
}

pub(super) fn build_sms_backend() -> EmuBackend {
    let rom = build_sms_test_rom();
    let sms = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &rom,
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .expect("SMS emulator should initialize");
    EmuBackend::from_sega8(sms, PathBuf::from("test.sms"))
}

pub(super) fn load_test_backend_with_shared_loader(
    system: ActiveSystem,
    rom_name: &str,
    rom: Vec<u8>,
) -> EmuBackend {
    let rom_path = PathBuf::from(rom_name);
    let loaded = load_backend_from_rom_source(
        system,
        &rom_path,
        &rom_path,
        Some(rom),
        BackendLoadConfig {
            sample_rate: Some(44_100),
            initial_input: Some((0x01, 0x02)),
            ..BackendLoadConfig::default()
        },
    )
    .expect("shared backend loader should initialize test ROM");
    loaded.backend
}

pub(super) fn assert_save_state_replay_is_deterministic(
    mut backend: EmuBackend,
    frames_before_checkpoint: usize,
    frames_after_checkpoint: usize,
) {
    step_frames(&mut backend, frames_before_checkpoint);

    let checkpoint_framebuffer = backend.framebuffer().to_vec();
    let checkpoint_state = backend
        .encode_state_bytes()
        .expect("backend should encode checkpoint save-state");

    step_frames(&mut backend, frames_after_checkpoint);

    let expected_framebuffer = backend.framebuffer().to_vec();
    let expected_state = backend
        .encode_state_bytes()
        .expect("backend should encode replay target save-state");

    backend
        .load_state_from_bytes(checkpoint_state)
        .expect("backend should restore checkpoint save-state");

    assert_eq!(
        backend.framebuffer(),
        checkpoint_framebuffer,
        "loading a save-state should restore the checkpoint framebuffer"
    );

    step_frames(&mut backend, frames_after_checkpoint);

    assert_eq!(
        backend.framebuffer(),
        expected_framebuffer,
        "replaying from a save-state should reproduce the same framebuffer"
    );
    assert_eq!(
        backend
            .encode_state_bytes()
            .expect("backend should encode replayed save-state"),
        expected_state,
        "replaying from a save-state should reproduce the same encoded state"
    );
}

pub(super) fn assert_runtime_audio_output_settings_do_not_affect_encoded_state(
    mut default_audio: EmuBackend,
    mut alternate_audio: EmuBackend,
) {
    default_audio.set_sample_rate(44_100);

    alternate_audio.set_sample_rate(96_000);
    alternate_audio.set_apu_sample_generation_enabled(false);
    alternate_audio.set_apu_channel_mutes(&[true, true, true, true, true, true]);

    step_frames(&mut default_audio, 3);
    step_frames(&mut alternate_audio, 3);

    assert_eq!(default_audio.frame_count(), alternate_audio.frame_count());
    assert_eq!(
        default_audio.framebuffer(),
        alternate_audio.framebuffer(),
        "runtime audio output settings should not affect video output for identical input"
    );

    let default_state = default_audio
        .encode_state_bytes()
        .expect("backend should encode default-audio save-state");
    let alternate_state = alternate_audio
        .encode_state_bytes()
        .expect("backend should encode alternate-audio save-state");

    if default_state != alternate_state {
        let first_diff = default_state
            .iter()
            .zip(&alternate_state)
            .position(|(left, right)| left != right);
        panic!(
            "runtime audio output settings changed encoded deterministic state: default_len={} alternate_len={} first_diff={first_diff:?}",
            default_state.len(),
            alternate_state.len()
        );
    }
}

pub(super) fn assert_backend_feature_contract(
    mut backend: EmuBackend,
    system: ActiveSystem,
    expected_save_ram_kind: SaveRamKind,
    expected_system_ram_len: usize,
    expected_video_ram_len: usize,
) {
    assert_eq!(backend.system(), system);
    assert_eq!(backend.save_ram_kind(), expected_save_ram_kind);
    assert_eq!(
        backend.has_battery(),
        expected_save_ram_kind.is_battery_backed()
    );
    assert_eq!(backend.system_ram_len(), expected_system_ram_len);
    assert_eq!(backend.video_ram_len(), expected_video_ram_len);
    assert_memory_regions(
        &backend.memory_regions(),
        system,
        expected_save_ram_kind,
        expected_system_ram_len,
        expected_video_ram_len,
        backend.framebuffer().len(),
    );
    assert_copyable_memory_regions(&mut backend);
    assert!(backend.supports_debugger());
    assert_eq!(
        backend.supports_opcode_history(),
        expected_opcode_history_support(system)
    );
    assert!(
        !backend
            .encode_state_bytes()
            .expect("backend should encode save-state")
            .is_empty()
    );
}

pub(super) fn assert_app_snapshot_core_features(
    mut backend: EmuBackend,
    expected_save_ram_kind: SaveRamKind,
    expected_system_ram_len: usize,
    expected_video_ram_len: usize,
) {
    let expected_system = backend.system();
    let expected_core_family = backend.core_family();
    let expected_framebuffer_len = backend.framebuffer().len();
    let data =
        EmuThread::collect_ui_snapshot(&mut backend, &snapshot_request(), reusable_buffers());
    let features = data
        .core_features
        .expect("app snapshot should expose core features");
    assert_eq!(features.core_family, expected_core_family);
    assert_eq!(features.save_ram_kind, expected_save_ram_kind);
    assert_eq!(
        features.has_battery,
        expected_save_ram_kind.is_battery_backed()
    );
    assert_eq!(features.system_ram_len, expected_system_ram_len);
    assert_eq!(features.video_ram_len, expected_video_ram_len);
    assert_memory_regions(
        &features.memory_regions,
        expected_system,
        expected_save_ram_kind,
        expected_system_ram_len,
        expected_video_ram_len,
        expected_framebuffer_len,
    );
    assert_eq!(
        features.input_features,
        crate::emu_backend::InputCapabilities::for_system(expected_system)
    );
    assert!(features.supports_save_states);
    assert!(features.supports_rewind);
    assert!(features.supports_debugger);
    assert_eq!(
        features.supports_opcode_history,
        expected_opcode_history_support(expected_system)
    );
    assert_eq!(
        features.cheat_features,
        crate::emu_backend::CheatCapabilities::for_system(expected_system)
    );
}

fn expected_opcode_history_support(system: ActiveSystem) -> bool {
    matches!(
        system,
        ActiveSystem::Gb
            | ActiveSystem::Gba
            | ActiveSystem::Nes
            | ActiveSystem::Ws
            | ActiveSystem::Sms
            | ActiveSystem::Gg
            | ActiveSystem::Sg
    )
}

fn assert_memory_regions(
    regions: &[MemoryRegionDescriptor],
    system: ActiveSystem,
    save_ram_kind: SaveRamKind,
    expected_system_ram_len: usize,
    expected_video_ram_len: usize,
    expected_framebuffer_len: usize,
) {
    assert_region(
        regions,
        "cpu",
        MemoryRegionKind::CpuAddressSpace,
        None,
        Some(expected_cpu_address_bits(system)),
    );
    assert_region(
        regions,
        "system_ram",
        MemoryRegionKind::SystemRam,
        Some(expected_system_ram_len),
        None,
    );
    assert_region(
        regions,
        "video_ram",
        MemoryRegionKind::VideoRam,
        Some(expected_video_ram_len),
        None,
    );
    assert_region(
        regions,
        "framebuffer",
        MemoryRegionKind::Framebuffer,
        Some(expected_framebuffer_len),
        None,
    );
    assert_eq!(
        regions.iter().any(|region| region.id == "save_ram"),
        save_ram_kind.has_ram()
    );
    if save_ram_kind.has_ram() {
        assert_region(
            regions,
            "save_ram",
            MemoryRegionKind::SaveRam,
            Some(save_ram_kind.size()),
            None,
        );
    }

    assert_extended_memory_regions(regions, system);
}

fn assert_extended_memory_regions(regions: &[MemoryRegionDescriptor], system: ActiveSystem) {
    match system {
        ActiveSystem::GameBoyAdvance => {
            assert_region(
                regions,
                "ewram",
                MemoryRegionKind::ExternalWorkRam,
                Some(zeff_gba_core::hardware::constants::EWRAM_SIZE),
                None,
            );
            assert_region(
                regions,
                "iwram",
                MemoryRegionKind::InternalWorkRam,
                Some(zeff_gba_core::hardware::constants::IWRAM_SIZE),
                None,
            );
            assert_region(
                regions,
                "palette_ram",
                MemoryRegionKind::PaletteRam,
                Some(zeff_gba_core::hardware::constants::PALETTE_RAM_SIZE),
                None,
            );
            assert_region(
                regions,
                "oam",
                MemoryRegionKind::Oam,
                Some(zeff_gba_core::hardware::constants::OAM_SIZE),
                None,
            );
            assert_region(
                regions,
                "io_registers",
                MemoryRegionKind::IoRegisters,
                Some(zeff_gba_core::hardware::constants::IO_SIZE),
                None,
            );
        }
        ActiveSystem::Nes => {
            assert_region(
                regions,
                "palette_ram",
                MemoryRegionKind::PaletteRam,
                Some(32),
                None,
            );
            assert_region(regions, "oam", MemoryRegionKind::Oam, Some(256), None);
            assert_no_region_kind(regions, MemoryRegionKind::IoRegisters);
        }
        ActiveSystem::MasterSystem | ActiveSystem::GameGear => {
            assert_region(
                regions,
                "palette_ram",
                MemoryRegionKind::PaletteRam,
                Some(zeff_sega8_core::hardware::constants::SMS_CRAM_SIZE),
                None,
            );
            assert_no_region_kind(regions, MemoryRegionKind::Oam);
            assert_no_region_kind(regions, MemoryRegionKind::IoRegisters);
        }
        _ => {
            assert_no_region_kind(regions, MemoryRegionKind::PaletteRam);
            assert_no_region_kind(regions, MemoryRegionKind::Oam);
            assert_no_region_kind(regions, MemoryRegionKind::IoRegisters);
        }
    }
}

fn assert_region(
    regions: &[MemoryRegionDescriptor],
    id: &str,
    kind: MemoryRegionKind,
    size: Option<usize>,
    address_bits: Option<u8>,
) {
    let region = regions
        .iter()
        .find(|region| region.id == id)
        .unwrap_or_else(|| panic!("missing memory region: {id}"));

    assert_eq!(region.kind, kind);
    assert_eq!(region.size, size);
    assert_eq!(region.address_bits, address_bits);
}

fn assert_no_region_kind(regions: &[MemoryRegionDescriptor], kind: MemoryRegionKind) {
    assert!(
        !regions.iter().any(|region| region.kind == kind),
        "unexpected memory region kind: {kind:?}"
    );
}

fn assert_copyable_memory_regions(backend: &mut EmuBackend) {
    let regions = backend.memory_regions();
    let mut copied = Vec::new();

    for region in regions {
        if region.kind == MemoryRegionKind::CpuAddressSpace {
            assert!(!region.copyable);
            assert!(
                backend.copy_memory_region(region.id, &mut copied).is_err(),
                "CPU address spaces should not be copied as finite memory regions"
            );
            continue;
        }

        assert!(
            region.copyable,
            "finite memory region '{}' should be marked copyable",
            region.id
        );
        let copied_region = backend
            .copy_memory_region(region.id, &mut copied)
            .unwrap_or_else(|err| panic!("copying memory region '{}' failed: {err}", region.id));
        assert_eq!(copied_region, region);
        assert_eq!(copied.len(), region.size.unwrap_or_default());

        if let Some(alias) = region.aliases.first() {
            let alias_region = backend
                .copy_memory_region(alias, &mut copied)
                .unwrap_or_else(|err| {
                    panic!(
                        "copying memory region '{}' through alias '{}' failed: {err}",
                        region.id, alias
                    )
                });
            assert_eq!(alias_region, region);
            assert_eq!(copied.len(), region.size.unwrap_or_default());
        }
    }

    assert!(
        backend
            .copy_memory_region("missing-region", &mut copied)
            .is_err()
    );
}

fn expected_cpu_address_bits(system: ActiveSystem) -> u8 {
    match system {
        ActiveSystem::GameBoyAdvance => 32,
        ActiveSystem::WonderSwan => 20,
        _ => 16,
    }
}

fn step_frames(backend: &mut EmuBackend, count: usize) {
    for _ in 0..count {
        backend.step_frame();
    }
}

fn snapshot_request() -> SnapshotRequest {
    SnapshotRequest {
        want_debug_info: false,
        want_perf_info: false,
        any_viewer_open: false,
        any_vram_viewer_open: false,
        show_oam_viewer: false,
        show_apu_viewer: false,
        show_disassembler: false,
        show_rom_info: false,
        show_memory_viewer: false,
        memory_view_start: 0,
        show_rom_viewer: false,
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

fn reusable_buffers() -> ReusableBuffers {
    ReusableBuffers {
        audio: None,
        vram: None,
        oam: None,
        memory_page: None,
        nes_chr: None,
        nes_nametable: None,
    }
}
