use super::*;
use zeff_md_core::{ControllerKind, ControllerPort, Region};

#[cfg(not(target_arch = "wasm32"))]
mod persistence;
mod state;

const ROM_BYTES: usize = 32 * 1024;
const PROGRAM_START: usize = 0x200;
const REGION_START: usize = 0x1f0;
const VDP_CONTROL: u32 = 0xc00004;
const LINE_TICKS: u64 = 3420;
const LOOP: [u16; 2] = [0x60fe, 0x4e71];

pub(super) fn rom(region: u8, words: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0; ROM_BYTES];
    bytes[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    bytes[4..8].copy_from_slice(&(PROGRAM_START as u32).to_be_bytes());
    bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    bytes[REGION_START] = region;
    for (index, word) in words.iter().enumerate() {
        let start = PROGRAM_START + index * 2;
        bytes[start..start + 2].copy_from_slice(&word.to_be_bytes());
    }
    bytes
}

fn load(bytes: Vec<u8>, config: BackendLoadConfig) -> EmuBackend {
    let path = PathBuf::from("synthetic.md");
    load_backend_from_rom_source(ActiveSystem::Md, &path, &path, Some(bytes), config)
        .expect("synthetic Mega Drive cartridge should load")
        .backend
}

fn machine(backend: &EmuBackend) -> &zeff_md_core::Emulator {
    let EmuBackend::Md(backend) = backend else {
        panic!("Mega Drive loader returned another backend");
    };
    &backend.emu
}

fn write_byte(words: &mut Vec<u16>, address: u32, value: u8) {
    words.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn write_word(words: &mut Vec<u16>, address: u32, value: u16) {
    words.extend([0x33fc, value, (address >> 16) as u16, address as u16]);
}

#[test]
fn md_loader_applies_header_region_override_and_sample_rate() {
    for (header, requested, expected) in [
        (b'U', None, Region::NtscUs),
        (b'E', None, Region::Pal),
        (b'J', None, Region::NtscJapan),
        (b'E', Some(Region::NtscUs), Region::NtscUs),
        (b'U', Some(Region::Pal), Region::Pal),
        (b'U', Some(Region::NtscJapan), Region::NtscJapan),
    ] {
        let backend = load(
            rom(header, &LOOP),
            BackendLoadConfig {
                md_region: requested,
                sample_rate: Some(48_000),
                ..BackendLoadConfig::default()
            },
        );
        assert_eq!(backend.system(), ActiveSystem::Md);
        assert_eq!(machine(&backend).region(), expected);
        assert_eq!(machine(&backend).sample_rate(), 48_000);
        assert_eq!(
            backend.nominal_frame_duration_ns(),
            (expected.frame_ticks() * 1_000_000_000 + expected.master_hz() / 2)
                / expected.master_hz()
        );
    }
}

#[test]
fn md_loader_preserves_source_paths_and_effective_rom_identity() {
    let bytes = rom(b'U', &LOOP);
    for (source, path) in [
        (PathBuf::from("direct.md"), PathBuf::from("direct.md")),
        (
            PathBuf::from("collection.zip"),
            PathBuf::from("collection.zip").join("folder/game.gen"),
        ),
    ] {
        let backend = load_backend_from_rom_source(
            ActiveSystem::Md,
            &source,
            &path,
            Some(bytes.clone()),
            BackendLoadConfig::default(),
        )
        .unwrap()
        .backend;
        assert_eq!(backend.source_path(), source);
        assert_eq!(backend.rom_path(), path);
        assert_eq!(backend.rom_hash(), zeff_firmware::sha256_bytes(&bytes));
    }
}

#[test]
fn md_backend_projects_each_host_button_to_both_six_button_ports() {
    let mut backend = load(rom(b'U', &LOOP), BackendLoadConfig::default());
    for port in [ControllerPort::One, ControllerPort::Two] {
        assert_eq!(
            machine(&backend).input().controller_kind(port),
            ControllerKind::SixButton
        );
        for (buttons, dpad, raw, extra) in [
            (1 << 6, 0, !(1 << 6), 0x0f),
            (1, 0, !(1 << 4), 0x0f),
            (1 << 1, 0, !(1 << 5), 0x0f),
            (1 << 4, 0, 0xff, 0x0b),
            (1 << 7, 0, 0xff, 0x0d),
            (1 << 5, 0, 0xff, 0x0e),
            (1 << 3, 0, !(1 << 7), 0x0f),
            (1 << 2, 0, 0xff, 0x07),
            (0, 1, !(1 << 3), 0x0f),
            (0, 1 << 1, !(1 << 2), 0x0f),
            (0, 1 << 2, !1, 0x0f),
            (0, 1 << 3, !(1 << 1), 0x0f),
        ] {
            backend.set_input(0, 0);
            backend.set_input_p2(0, 0);
            match port {
                ControllerPort::One => backend.set_input(buttons, dpad),
                ControllerPort::Two => backend.set_input_p2(buttons, dpad),
            }
            let input = machine(&backend).input();
            assert_eq!(input.controller_raw(port), raw);
            assert_eq!(input.controller_extra_raw(port), extra);
            let other = match port {
                ControllerPort::One => ControllerPort::Two,
                ControllerPort::Two => ControllerPort::One,
            };
            assert_eq!(input.controller_raw(other), 0xff);
            assert_eq!(input.controller_extra_raw(other), 0x0f);
        }
    }
    let backend = load(
        rom(b'U', &LOOP),
        BackendLoadConfig {
            md_controller: ControllerKind::ThreeButton,
            ..BackendLoadConfig::default()
        },
    );
    for port in [ControllerPort::One, ControllerPort::Two] {
        assert_eq!(
            machine(&backend).input().controller_kind(port),
            ControllerKind::ThreeButton
        );
    }
}

#[test]
fn md_backend_delivers_cpu_generated_left_dac_and_stereo_pcm() {
    let mut words = Vec::new();
    for (address, value) in [
        (0xa11100, 1),
        (0xa11200, 1),
        (0xa04002, 0xb6),
        (0xa04003, 0x80),
        (0xa04000, 0x2a),
        (0xa04001, 0xff),
        (0xa04000, 0x2b),
        (0xa04001, 0x80),
    ] {
        write_byte(&mut words, address, value);
    }
    words.extend(LOOP);
    for region in *b"UE" {
        let mut backend = load(
            rom(region, &words),
            BackendLoadConfig {
                sample_rate: Some(48_000),
                ..BackendLoadConfig::default()
            },
        );
        backend.step_frame();
        assert!(!backend.is_suspended());
        assert_eq!(machine(&backend).opn2_snapshot().dac, 0xff);
        assert!(machine(&backend).opn2_snapshot().dac_enabled);
        let mut samples = Vec::new();
        backend.drain_audio_samples_into(&mut samples);
        assert!(samples.len() > 1_000);
        assert_eq!(samples.len() % 2, 0);
        assert!(
            samples
                .as_chunks::<2>()
                .0
                .iter()
                .any(|frame| frame[0] == 8128.0 / 32768.0)
        );
        assert!(
            samples
                .as_chunks::<2>()
                .0
                .iter()
                .all(|frame| frame[1] == 0.0)
        );
        samples.clear();
        backend.drain_audio_samples_into(&mut samples);
        assert!(samples.is_empty());
    }
}

#[test]
fn md_backend_exposes_cpu_selected_h40_and_pal_v30_geometry() {
    for (header, mode, height) in [(b'U', 0x04, 224), (b'E', 0x0c, 240)] {
        let mut words = Vec::new();
        write_word(&mut words, VDP_CONTROL, 0x8c81);
        write_word(&mut words, VDP_CONTROL, 0x8100 | mode);
        words.extend(LOOP);
        let mut backend = load(rom(header, &words), BackendLoadConfig::default());
        assert_eq!(backend.display_framebuffer().1, Some((256, 224)));
        backend.step_frame();
        assert!(!backend.is_suspended());
        let (pixels, dimensions) = backend.display_framebuffer();
        assert_eq!(dimensions, Some((320, height)));
        assert_eq!(pixels.len(), 320 * height as usize * 4);
        assert_eq!(machine(&backend).video_error(), None);
    }
}

#[test]
fn md_backend_paces_cpu_selected_interlace_fields_and_restores_progressive() {
    for (header, short_lines) in [(b'U', 262_u64), (b'J', 262), (b'E', 312)] {
        let mut words = Vec::new();
        write_word(&mut words, VDP_CONTROL, 0x8c87);
        write_word(&mut words, VDP_CONTROL, 0x8144);
        words.extend(LOOP);
        let mut backend = load(rom(header, &words), BackendLoadConfig::default());
        let region = machine(&backend).region();
        let mut deadline = 0;
        for field in 1..=4 {
            deadline += (short_lines + field % 2) * LINE_TICKS;
            backend.step_frame();
            assert!(!backend.is_suspended());
            assert_eq!(backend.frame_count(), field);
            let now = machine(&backend).timing_snapshot().now().get();
            assert!((deadline..deadline + 100).contains(&now));
            assert_eq!(
                backend.nominal_frame_duration_ns(),
                ((2 * short_lines + 1) * LINE_TICKS * 1_000_000_000 + region.master_hz())
                    / (2 * region.master_hz())
            );
            if field > 1 {
                let (pixels, dimensions) = backend.display_framebuffer();
                assert_eq!(dimensions, Some((320, 448)));
                assert_eq!(pixels.len(), 320 * 448 * 4);
            }
        }
        let EmuBackend::Md(md) = &mut backend else {
            unreachable!()
        };
        while md.emu.queue_vdp_register(12, 0x81).is_err() {
            md.emu.step_instruction().unwrap();
        }
        for _ in 0..3 {
            backend.step_frame();
        }
        assert!(!backend.is_suspended());
        assert_eq!(backend.display_framebuffer().1, Some((320, 224)));
        assert_eq!(
            backend.nominal_frame_duration_ns(),
            (region.frame_ticks() * 1_000_000_000 + region.master_hz() / 2) / region.master_hz()
        );
        backend.reset();
        assert_eq!(backend.frame_count(), 0);
        assert_eq!(backend.display_framebuffer().1, Some((256, 224)));
    }
}

#[test]
fn md_backend_paces_fields_before_display_and_pause_freezes_machine() {
    for header in *b"UE" {
        let mut backend = load(rom(header, &LOOP), BackendLoadConfig::default());
        for field in 1..=3 {
            backend.step_frame();
            assert!(!backend.is_suspended());
            assert_eq!(backend.frame_count(), field);
        }
        let paused = machine(&backend).clone();
        assert!(!backend.supports_execution_controls());
        backend.debug_suspend();
        assert_eq!(machine(&backend), &paused);
        assert!(backend.take_runtime_fault().is_none());
        backend.reset();
        assert!(!backend.is_suspended());
        assert_eq!(backend.frame_count(), 0);
        backend.step_frame();
        assert_eq!(backend.frame_count(), 1);
    }
}

#[test]
fn md_backend_fault_is_reported_once_and_reset_recovers() {
    let mut words = Vec::new();
    write_byte(&mut words, 0xb00000, 0xaa);
    words.extend(LOOP);
    let mut backend = load(rom(b'U', &words), BackendLoadConfig::default());
    backend.step_frame();
    assert!(backend.is_suspended());
    assert!(
        backend
            .take_runtime_fault()
            .unwrap()
            .contains("Mega Drive machine fault")
    );
    assert!(backend.take_runtime_fault().is_none());
    let faulted = machine(&backend).clone();
    backend.step_frame();
    assert_eq!(machine(&backend), &faulted);
    backend.reset();
    assert!(!backend.is_suspended());
    assert!(backend.take_runtime_fault().is_none());
    assert_eq!(machine(&backend).registers().pc, PROGRAM_START as u32);
    backend.step_frame();
    assert!(backend.take_runtime_fault().is_some());
}

#[test]
fn md_backend_rejects_malformed_state_transactionally_and_advertises_limits() {
    let mut backend = load(rom(b'U', &LOOP), BackendLoadConfig::default());
    backend.set_input(0x51, 0x03);
    backend.step_frame();
    let before = machine(&backend).clone();
    assert!(backend.encode_state_bytes().is_ok());
    assert_eq!(machine(&backend), &before);
    assert!(backend.load_state_from_bytes(vec![0; 64]).is_err());
    assert_eq!(machine(&backend), &before);
    assert!(!backend.is_suspended());
    assert!(backend.take_runtime_fault().is_none());
    let caps = backend.capabilities();
    assert!(caps.supports_audio);
    assert!(caps.supports_save_states);
    assert!(caps.supports_state_capture);
    assert!(!caps.supports_rewind);
    assert!(!caps.supports_replay);
    assert!(!caps.supports_debugger);
    assert!(!caps.supports_execution_controls);
    assert!(!caps.supports_opcode_history);
    assert!(!caps.supports_cheats);
    assert!(!caps.supports_guest_calls);
    assert_eq!(caps.input_features.max_players, 2);
    assert_eq!(caps.save_ram_kind, SaveRamKind::none());
}
