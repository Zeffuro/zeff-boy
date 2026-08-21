use super::*;

const RESET_PC: u16 = 0xE000;

fn rom_with_program(program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[..program.len()].copy_from_slice(program);
    rom[0x1FFE..0x2000].copy_from_slice(&RESET_PC.to_le_bytes());
    rom
}

fn backend_with_board(board: PceHuCardBoard, image_len: usize) -> PceBackend {
    let mut rom = rom_with_program(&[0xA9, 0x5A, 0x8D, 0x00, 0x40]);
    rom.resize(image_len, 0xEA);
    let descriptor = PceCartridgeDescriptor::default().with_hucard_board(board);
    let machine =
        PceMachine::with_cartridge_and_controller(rom, descriptor, ControllerPort::two_button())
            .unwrap();
    let mut backend = PceBackend {
        machine,
        paths: BackendPaths::new(PathBuf::from("board.pce")),
        rom_hash: [0; 32],
        framebuffer: vec![0; PCE_PRESENTED_RGBA_BYTES].into_boxed_slice(),
        frame_count: 0,
        pending_runtime_fault: None,
        overscan_mode: PceOverscanMode::default(),
        palette_mode: PcePaletteMode::default(),
        pce_controller_mode: PceControllerMode::Automatic,
        mouse_host_buttons: PadButtons::empty(),
    };
    backend.project_presented_frame();
    backend
}

#[test]
fn structural_sf2_image_is_admitted_without_relaxing_plain_cards() {
    let mut rom = rom_with_program(&[0xEA]);
    rom.resize(zeff_pce_core::hardware::SF2_CE_HUCARD_IMAGE_LEN, 0xEA);
    let backend = PceBackend::new(rom, PathBuf::from("sf2.pce")).unwrap();
    assert_eq!(backend.hucard_board(), PceHuCardBoard::Sf2Ce);
    assert_eq!(backend.hucard_rom().len(), 0x28_0000);

    let oversized_plain = vec![0; 0x10_0000 + 0x2000];
    assert!(PceBackend::new(oversized_plain, PathBuf::from("unknown.pce")).is_err());
}

#[test]
fn exact_lemmings_disc_automatically_selects_mouse_but_force_pad_wins() {
    let mut backend = backend_with_board(PceHuCardBoard::SystemCardV3, 0x40_000);
    backend.rom_hash = LEMMINGS_JAPAN_CANONICAL_DISC_SHA256;

    backend.set_pce_mouse_state(PceControllerMode::Automatic, 1, 2, 1);
    assert!(matches!(
        backend.machine.devices().controller().device(),
        zeff_pce_core::hardware::ControllerDevice::Mouse(_)
    ));

    backend.set_pce_mouse_state(PceControllerMode::TwoButton, 0, 0, 0);
    assert!(matches!(
        backend.machine.devices().controller().device(),
        zeff_pce_core::hardware::ControllerDevice::TwoButton(_)
    ));
}

#[test]
fn exact_deden_disc_automatically_selects_multitap_with_independent_second_pad() {
    let mut backend = backend_with_board(PceHuCardBoard::SystemCardV3, 0x40_000);
    backend.rom_hash = TENGAI_MAKYOU_DEDEN_NO_KABUKI_DEN_CANONICAL_DISC_SHA256;

    backend.set_pce_mouse_state(PceControllerMode::Automatic, 0, 0, 0);
    backend.set_input(1, 0);
    backend.set_input_p2(2, 0);

    let zeff_pce_core::hardware::ControllerDevice::Multitap(multitap) =
        backend.machine.devices().controller().device()
    else {
        panic!("Deden no Kabuki-den did not select the multitap");
    };
    let zeff_pce_core::hardware::MultitapDevice::TwoButton(p1) =
        multitap.port(zeff_pce_core::hardware::MultitapPort::One)
    else {
        panic!("multitap port 1 is not a two-button pad");
    };
    let zeff_pce_core::hardware::MultitapDevice::TwoButton(p2) =
        multitap.port(zeff_pce_core::hardware::MultitapPort::Two)
    else {
        panic!("multitap port 2 is not a two-button pad");
    };
    assert_eq!(p1.buttons(), PadButtons::I);
    assert_eq!(p2.buttons(), PadButtons::II);
}

#[test]
fn pceas_development_header_is_validated_and_removed_before_hashing() {
    let mut payload = rom_with_program(&[0xEA]);
    payload.resize(3 * HUCARD_BANK_LEN, 0xEA);
    let expected_hash = zeff_firmware::sha256_bytes(&payload);
    let mut headered = vec![0; PCEAS_HEADER_LEN];
    headered[0] = 3;
    headered.extend_from_slice(&payload);

    let backend = PceBackend::new(headered, PathBuf::from("source-test.pce")).unwrap();
    assert_eq!(backend.hucard_rom(), payload);
    assert_eq!(backend.rom_hash(), expected_hash);

    let mut invalid = vec![0; PCEAS_HEADER_LEN];
    invalid[0] = 2;
    invalid.extend_from_slice(&payload);
    assert!(PceBackend::new(invalid, PathBuf::from("invalid.pce")).is_err());
}

#[test]
fn populous_ram_is_nonbattery_copyable_mapper_ram_without_state_capabilities() {
    let mut backend = backend_with_board(
        PceHuCardBoard::Populous,
        zeff_pce_core::hardware::POPULOUS_HUCARD_IMAGE_LEN,
    );
    assert_eq!(
        backend.save_ram_kind(),
        SaveRamKind::mapper_ram_unknown(POPULOUS_HUCARD_RAM_LEN)
    );
    assert!(!backend.save_ram_kind().is_battery_backed());
    assert!(!backend.supports_save_states());
    assert!(!backend.supports_rewind());
    assert!(!backend.supports_replay());
    assert_eq!(backend.flush_battery_sram().unwrap(), None);

    backend
        .machine
        .cpu_mut()
        .cpu_mut()
        .set_mapping_register(2, 0x40);
    backend.machine.step_boundary().unwrap();
    backend.machine.step_boundary().unwrap();
    let mut ram = Vec::new();
    assert_eq!(
        backend.copy_memory_region("save_ram", &mut ram).unwrap(),
        MemoryRegionDescriptor::save_ram(POPULOUS_HUCARD_RAM_LEN)
    );
    assert_eq!(ram.len(), POPULOUS_HUCARD_RAM_LEN);
    assert_eq!(ram[0], 0x5A);
}

#[test]
fn cd_backup_ram_is_formatted_battery_backed_and_copyable() {
    let mut system_card = vec![0xEA; zeff_pce_core::hardware::SYSTEM_CARD_V1_V2_IMAGE_LEN];
    system_card[0x1FFE..0x2000].copy_from_slice(&RESET_PC.to_le_bytes());
    let disc = CdDisc::new(vec![
        zeff_pce_core::hardware::CdTrack::from_index1_data(
            1,
            4,
            None,
            0,
            zeff_pce_core::hardware::CdTrackMode::Mode1_2048,
            vec![0; zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
        )
        .unwrap(),
    ])
    .unwrap();
    let mut backend = PceBackend::new_cdrom2(
        system_card,
        disc,
        PceCdBackendConfig {
            system_card_board: PceHuCardBoard::SystemCardV1V2,
            cue_path: PathBuf::from("disc.cue"),
            source_path: PathBuf::from("disc.cue"),
            content_hash: [0; 32],
            console_wiring: PceConsoleWiring::PcEngine,
        },
    )
    .unwrap();

    assert_eq!(
        backend.save_ram_kind(),
        SaveRamKind::known_battery_backed(CDROM2_BRAM_LEN)
    );
    let mut ram = Vec::new();
    backend.copy_memory_region("save_ram", &mut ram).unwrap();
    assert_eq!(&ram[..8], b"HUBM\x00\xA0\x10\x80");

    let replacement = vec![0x5A; CDROM2_BRAM_LEN];
    backend.load_cd_bram(&replacement).unwrap();
    backend.copy_memory_region("save_ram", &mut ram).unwrap();
    assert_eq!(ram, replacement);
    assert!(
        backend
            .load_cd_bram(&replacement[..CDROM2_BRAM_LEN - 1])
            .is_err()
    );
}

fn pixel(frame: &[u8], x: usize, y: usize) -> [u8; 4] {
    frame[(y * PCE_PRESENTED_WIDTH + x) * 4..][..4]
        .try_into()
        .unwrap()
}

#[test]
fn projection_maps_variable_active_rows_without_sampling_padding() {
    let mut source = vec![0; 4 * 2 * 4];
    let colors = [
        [1, 0, 0, 0xFF],
        [2, 0, 0, 0xFF],
        [3, 0, 0, 0xFF],
        [4, 0, 0, 0xFF],
        [5, 0, 0, 0xFF],
        [6, 0, 0, 0xFF],
        [0xEE, 0, 0, 0xFF],
        [0xEF, 0, 0, 0xFF],
    ];
    for (pixel, color) in source.chunks_exact_mut(4).zip(colors) {
        pixel.copy_from_slice(&color);
    }
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 1,
            active: true,
        },
        ProjectionRow {
            active_x_origin: 0,
            active_width: 2,
            pixel_clock_divisor: 1,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_sgx_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);

    assert_eq!(pixel(&output, 0, 0), colors[0]);
    assert_eq!(pixel(&output, 639, 239), colors[3]);
    assert_eq!(pixel(&output, 159, 240), colors[4]);
    assert_eq!(pixel(&output, 160, 240), colors[5]);
    assert_eq!(pixel(&output, 319, 240), colors[5]);
    assert_eq!(pixel(&output, 320, 240), OPAQUE_BLACK);
    assert_eq!(pixel(&output, 639, 479), OPAQUE_BLACK);
}

#[test]
fn base_projection_scales_each_active_row_without_black_side_bars() {
    let mut source = vec![0; 4 * 2 * 4];
    let colors = [
        [1, 0, 0, 0xFF],
        [2, 0, 0, 0xFF],
        [3, 0, 0, 0xFF],
        [4, 0, 0, 0xFF],
        [5, 0, 0, 0xFF],
        [6, 0, 0, 0xFF],
        [0xEE, 0, 0, 0xFF],
        [0xEF, 0, 0, 0xFF],
    ];
    for (pixel, color) in source.chunks_exact_mut(4).zip(colors) {
        pixel.copy_from_slice(&color);
    }
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 4,
            active: true,
        },
        ProjectionRow {
            active_x_origin: 7,
            active_width: 2,
            pixel_clock_divisor: 2,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_base_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);

    assert_eq!(pixel(&output, 0, 0), colors[0]);
    assert_eq!(pixel(&output, 639, 0), colors[3]);
    assert_eq!(pixel(&output, 0, 479), colors[4]);
    assert_eq!(pixel(&output, 319, 479), colors[4]);
    assert_eq!(pixel(&output, 320, 479), colors[5]);
    assert_eq!(pixel(&output, 639, 479), colors[5]);
}

#[test]
fn base_projection_preserves_the_complete_programmed_active_span() {
    const ACTIVE_WIDTH: usize = 352;
    const ACTIVE_HEIGHT: usize = 240;
    let mut source = vec![0; PCE_ACTIVE_FRAME_WIDTH * ACTIVE_HEIGHT * 4];
    for y in 0..ACTIVE_HEIGHT {
        for x in 0..ACTIVE_WIDTH {
            let offset = (y * PCE_ACTIVE_FRAME_WIDTH + x) * 4;
            source[offset..offset + 4].copy_from_slice(&[x as u8, y as u8, 0x40, 0xFF]);
        }
    }
    let rows = vec![
        ProjectionRow {
            active_x_origin: 0,
            active_width: ACTIVE_WIDTH,
            pixel_clock_divisor: 4,
            active: true,
        };
        ACTIVE_HEIGHT
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_base_rgba_rows(
        &source,
        PCE_ACTIVE_FRAME_WIDTH,
        &rows,
        Some((0, ACTIVE_HEIGHT)),
        &mut output,
    );

    assert_eq!(pixel(&output, 0, 0), [0, 0, 0x40, 0xFF]);
    assert_eq!(pixel(&output, 639, 479), [95, 239, 0x40, 0xFF]);
}

#[test]
fn projection_aligns_rows_in_one_master_dot_domain() {
    let mut source = vec![0; 4 * 2 * 4];
    let colors = [
        [1, 0, 0, 0xFF],
        [2, 0, 0, 0xFF],
        [0xA0, 0, 0, 0xFF],
        [4, 0, 0, 0xFF],
        [5, 0, 0, 0xFF],
        [6, 0, 0, 0xFF],
        [0xB0, 0, 0, 0xFF],
        [8, 0, 0, 0xFF],
    ];
    for (pixel, color) in source.chunks_exact_mut(4).zip(colors) {
        pixel.copy_from_slice(&color);
    }
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 4,
            active: true,
        },
        ProjectionRow {
            active_x_origin: 2,
            active_width: 4,
            pixel_clock_divisor: 2,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_sgx_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);

    assert_eq!(pixel(&output, 320, 0), colors[2]);
    assert_eq!(pixel(&output, 320, 479), colors[6]);
    assert_eq!(pixel(&output, 0, 479), OPAQUE_BLACK);
    assert_eq!(pixel(&output, 639, 479), OPAQUE_BLACK);
}

#[test]
fn projection_keeps_empty_and_inactive_rows_opaque_black() {
    let source = vec![0x7F; 4 * 2 * 4];
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 1,
            active: false,
        },
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 1,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_sgx_rgba_rows(&source, 4, &rows, None, &mut output);
    assert!(output.chunks_exact(4).all(|pixel| pixel == OPAQUE_BLACK));

    project_sgx_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);
    assert_eq!(pixel(&output, 10, 10), OPAQUE_BLACK);
    assert_eq!(pixel(&output, 10, 470), [0x7F; 4]);
}

#[test]
fn standard_host_input_maps_to_two_button_pad() {
    let mut backend = PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("pad.pce")).unwrap();

    backend.set_input(0b1101, 0b1011);

    let pad = backend.machine.devices().controller().device();
    let zeff_pce_core::hardware::ControllerDevice::TwoButton(pad) = pad else {
        panic!("frontend must construct a standard two-button pad");
    };
    assert_eq!(
        pad.buttons(),
        PadButtons::I
            | PadButtons::SELECT
            | PadButtons::RUN
            | PadButtons::RIGHT
            | PadButtons::LEFT
            | PadButtons::DOWN
    );
}

#[test]
fn explicit_console_wiring_overrides_auto_detection() {
    let backend = PceBackend::new_with_console_wiring(
        rom_with_program(&[0xEA]),
        PathBuf::from("override.pce"),
        PceConsoleWiring::TurboGrafx16,
    )
    .unwrap();

    assert_eq!(
        backend.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
}

#[test]
fn curated_wiring_hash_propagates_for_direct_and_archive_paths() {
    let magical_chase_sha256 = [
        0xC5, 0xA3, 0x9C, 0x9D, 0x9B, 0x2D, 0x75, 0x32, 0x44, 0x81, 0x6E, 0xAF, 0xD6, 0x8F, 0x50,
        0x4A, 0x85, 0x59, 0x08, 0xEE, 0xBA, 0xB1, 0xB1, 0xC8, 0xFE, 0xA2, 0xBB, 0xF7, 0xA4, 0xA8,
        0x13, 0xC7,
    ];
    let direct = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("magical.pce")),
        None,
        None,
        magical_chase_sha256,
    )
    .unwrap();
    let archive = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::with_source_path(
            PathBuf::from("cards.zip").join("magical.pce"),
            PathBuf::from("cards.zip"),
        ),
        None,
        None,
        magical_chase_sha256,
    )
    .unwrap();
    let explicit_pce = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("magical.pce")),
        Some(PceConsoleWiring::PcEngine),
        None,
        magical_chase_sha256,
    )
    .unwrap();

    assert_eq!(
        direct.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
    assert_eq!(
        archive.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
    assert_eq!(archive.source_path(), Path::new("cards.zip"));
    assert_eq!(
        explicit_pce.machine.devices().console_wiring(),
        PceConsoleWiring::PcEngine
    );
}

#[test]
fn read_only_debug_surface_reports_registers_and_rom_bytes() {
    let backend = PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("debug.pce")).unwrap();
    assert_eq!(backend.debug_cpu_snapshot().registers().pc, RESET_PC);
    assert_eq!(backend.debug_peek8(0), 0xEA);
    assert_eq!(
        backend.memory_regions()[0],
        MemoryRegionDescriptor::read_only_cpu_address_space(16)
    );
}

#[test]
fn supergrafx_direct_and_archive_profiles_expose_dynamic_work_ram() {
    let madou_sha256 = [
        0x9B, 0x57, 0xCD, 0xF0, 0xD0, 0xB1, 0x10, 0xF4, 0x12, 0x8B, 0x86, 0x34, 0x19, 0xD5, 0xBE,
        0x99, 0xA3, 0x70, 0x8B, 0xFB, 0x11, 0xCF, 0xBE, 0x16, 0x96, 0xF2, 0x54, 0x49, 0xB9, 0x91,
        0x02, 0x6D,
    ];
    let direct = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("madou.pce")),
        None,
        None,
        madou_sha256,
    )
    .unwrap();
    let mut archive = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::with_source_path(PathBuf::from("madou.pce"), PathBuf::from("cards.zip")),
        None,
        None,
        madou_sha256,
    )
    .unwrap();

    for backend in [&direct, &archive] {
        assert_eq!(
            backend.machine.hardware_topology(),
            zeff_pce_core::hardware::PceHardwareTopology::SuperGrafx
        );
        assert_eq!(
            backend.machine.devices().psg().revision(),
            zeff_pce_core::hardware::PsgRevision::HuC6280A
        );
        assert_eq!(
            backend.system_ram_len(),
            zeff_pce_core::hardware::SUPERGRAFX_WORK_RAM_LEN
        );
        assert_eq!(
            backend
                .memory_regions()
                .iter()
                .find(|region| region.kind == MemoryRegionKind::SystemRam)
                .and_then(|region| region.size),
            Some(zeff_pce_core::hardware::SUPERGRAFX_WORK_RAM_LEN)
        );
    }
    assert_eq!(archive.source_path(), Path::new("cards.zip"));
    let mut ram = Vec::new();
    archive.copy_memory_region("system_ram", &mut ram).unwrap();
    assert_eq!(ram.len(), zeff_pce_core::hardware::SUPERGRAFX_WORK_RAM_LEN);
}

#[test]
fn audio_backend_drains_stereo_psg_samples_at_the_requested_rate() {
    let mut backend =
        PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("audio.pce")).unwrap();
    let psg = backend.machine.devices_mut().psg_mut();
    let port = zeff_pce_core::hardware::PsgPort::from_offset;
    psg.write_port(port(0), 0);
    psg.write_port(port(1), 0xFF);
    psg.write_port(port(5), 0xFF);
    for value in 0..32 {
        psg.write_port(port(6), value);
    }
    psg.write_port(port(2), 1);
    psg.write_port(port(4), 0x9F);
    backend
        .machine
        .devices_mut()
        .advance_master_ticks(1_365 * 262);

    let mut samples = Vec::new();
    backend.drain_audio_samples_into(&mut samples);
    assert_eq!(samples.len(), 734 * 2);
    assert!(samples.iter().any(|sample| *sample != 0.0));
    assert_eq!(backend.audio_topology().unwrap().channels.len(), 6);
}

fn configure_test_tone(backend: &mut PceBackend) {
    let psg = backend.machine.devices_mut().psg_mut();
    let port = zeff_pce_core::hardware::PsgPort::from_offset;
    psg.write_port(port(0), 0);
    psg.write_port(port(1), 0xFF);
    psg.write_port(port(5), 0xFF);
    for value in 0..32 {
        psg.write_port(port(6), value);
    }
    psg.write_port(port(2), 1);
    psg.write_port(port(4), 0x9F);
}

fn run_audio_frames(
    backend: &mut PceBackend,
    frames: usize,
    lines_263: bool,
) -> (Vec<usize>, Vec<f32>) {
    if lines_263 {
        backend
            .machine
            .devices_mut()
            .vce_mut()
            .write_port(zeff_pce_core::hardware::VcePort::from_offset(0), 0x04);
    }
    let mut counts = Vec::with_capacity(frames);
    let mut samples = Vec::new();
    for _ in 0..frames {
        backend.step_frame();
        let mut frame_samples = Vec::new();
        backend.drain_audio_samples_into(&mut frame_samples);
        assert_eq!(frame_samples.len() % 2, 0);
        assert!(frame_samples.iter().all(|sample| sample.is_finite()));
        counts.push(frame_samples.len() / 2);
        samples.extend(frame_samples);
    }
    (counts, samples)
}

#[test]
fn multi_frame_audio_drains_preserve_fractional_cadence_and_continuity() {
    let mut drained_each_frame = PceBackend::new(
        rom_with_program(&[0xEA]),
        PathBuf::from("audio-frames-a.pce"),
    )
    .unwrap();
    let mut drained_once = PceBackend::new(
        rom_with_program(&[0xEA]),
        PathBuf::from("audio-frames-b.pce"),
    )
    .unwrap();
    configure_test_tone(&mut drained_each_frame);
    configure_test_tone(&mut drained_once);

    let (counts, samples) = run_audio_frames(&mut drained_each_frame, 120, false);
    let (_, one_drain_samples) = run_audio_frames(&mut drained_once, 120, false);
    assert_eq!(samples, one_drain_samples);
    assert_eq!(counts.iter().sum::<usize>(), 88_120);
    assert!(counts.iter().all(|count| matches!(count, 734 | 735)));
    assert!(samples.iter().any(|sample| *sample != 0.0));

    let mut lines_263 =
        PceBackend::new(rom_with_program(&[0xEA]), PathBuf::from("audio-263.pce")).unwrap();
    configure_test_tone(&mut lines_263);
    let (counts, _) = run_audio_frames(&mut lines_263, 120, true);
    assert_eq!(counts.iter().sum::<usize>(), 88_456);
    assert!(counts.iter().all(|count| matches!(count, 737 | 738)));
}

#[test]
fn machine_error_retains_frame_and_delivers_only_the_first_fault() {
    let mut backend = PceBackend::new(
        rom_with_program(&[0x03, 0x05, 0x13, 0x10, 0x23, 0x00, 0x80, 0xFE]),
        PathBuf::from("unsupported.pce"),
    )
    .unwrap();
    backend.framebuffer.fill(0xA5);
    let before = backend.framebuffer.to_vec();

    backend.step_frame();

    assert_eq!(&*backend.framebuffer, before);
    let fault = backend.take_runtime_fault().unwrap();
    assert!(fault.contains("ExternalVceSyncNeedsHorizontalScheduler"));
    assert_eq!(backend.take_runtime_fault(), None);
    backend.step_frame();
    assert_eq!(backend.take_runtime_fault(), None);
    assert_eq!(&*backend.framebuffer, before);
}
