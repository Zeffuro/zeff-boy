use super::*;

#[test]
fn libretro_registers_pce_hucards_with_fixed_host_geometry() {
    let extensions = crate::callbacks::VALID_EXTENSIONS
        .to_str()
        .expect("valid extensions should be UTF-8");

    assert!(extensions.split('|').any(|entry| entry == "pce"));
    let state = CoreState::from_rom(&pce_rom(), "test.pce").expect("PCE HuCard should load");
    assert!(matches!(state.core, ActiveCore::Pce(_)));
    assert_eq!(state.system_label(), "PC Engine");
    assert_eq!(state.video_geometry().base_width, 640);
    assert_eq!(state.video_geometry().base_height, 480);
    assert_eq!(state.video_geometry().max_width, 640);
    assert_eq!(state.video_geometry().max_height, 480);
    assert_eq!(state.video_geometry().aspect_ratio, 4.0 / 3.0);
}

#[test]
fn pce_libretro_normalizes_pceas_headers_before_catalog_hashing() {
    let rom = pce_rom();
    let plain = CoreState::from_rom(&rom, "plain.pce").unwrap();
    let mut headered = vec![0; 0x200];
    headered[0] = 1;
    headered.extend_from_slice(&rom);
    let headered = CoreState::from_rom(&headered, "headered.pce").unwrap();

    let (ActiveCore::Pce(plain), ActiveCore::Pce(headered)) = (&plain.core, &headered.core) else {
        panic!("expected PCE hosts");
    };
    assert_eq!(headered.image_sha256(), plain.image_sha256());
    assert_eq!(
        headered.machine().hucard_board(),
        plain.machine().hucard_board()
    );
    assert_eq!(
        headered.machine().hardware_topology(),
        plain.machine().hardware_topology()
    );
}

#[test]
fn pce_libretro_formats_exact_640_by_480_xrgb8888_and_rgb565_frames() {
    let mut state = CoreState::from_rom(&pce_rom(), "video.pce").unwrap();
    state.step_frame();
    let ActiveCore::Pce(host) = &mut state.core else {
        panic!("expected PCE host");
    };
    let rgba = host.framebuffer().to_vec();

    let xrgb = state.framebuffer_as_xrgb8888().to_vec();
    let rgb565 = state.framebuffer_as_rgb565().to_vec();
    assert_eq!(xrgb.len(), 640 * 480 * 4);
    assert_eq!(rgb565.len(), 640 * 480 * 2);

    let mut expected_xrgb = Vec::with_capacity(xrgb.len());
    let mut expected_rgb565 = Vec::with_capacity(rgb565.len());
    for source in rgba.as_chunks::<4>().0 {
        expected_xrgb.extend_from_slice(&[source[2], source[1], source[0], 0]);
        expected_rgb565.extend_from_slice(
            &(((u16::from(source[0]) >> 3) << 11)
                | ((u16::from(source[1]) >> 2) << 5)
                | (u16::from(source[2]) >> 3))
                .to_le_bytes(),
        );
    }
    assert_eq!(xrgb, expected_xrgb);
    assert_eq!(rgb565, expected_rgb565);
}

#[test]
fn pce_native_output_uses_tight_callback_buffers_after_geometry_acceptance() {
    let mut state = CoreState::from_rom(&pce_rom(), "native-video.pce").unwrap();
    state.step_frame();
    let descriptor = match &state.core {
        ActiveCore::Pce(host) => host.native_frame_descriptor().unwrap(),
        _ => unreachable!(),
    };
    let dimensions = (descriptor.width(), descriptor.height());
    assert_eq!(
        state.pending_pce_native_output_dimensions(),
        Some(Some(dimensions))
    );
    state.apply_pce_native_output_dimensions(Some(dimensions), true);
    let geometry = state.video_geometry();
    assert_eq!((geometry.base_width, geometry.base_height), (256, 242));
    assert_eq!((geometry.max_width, geometry.max_height), (640, 480));
    assert_eq!(state.framebuffer_as_xrgb8888().len(), 256 * 242 * 4);
    assert_eq!(state.framebuffer_as_rgb565().len(), 256 * 242 * 2);

    state.apply_pce_native_output_dimensions(Some(dimensions), false);
    let geometry = state.video_geometry();
    assert_eq!((geometry.base_width, geometry.base_height), (640, 480));
    assert_eq!(state.framebuffer_as_xrgb8888().len(), 640 * 480 * 4);
}
