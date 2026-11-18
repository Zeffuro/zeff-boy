use super::*;
use zeff_md_core::{ControllerKind, ControllerPort, Region};

pub(super) fn rom(region: u8, words: &[u16], save: bool) -> Vec<u8> {
    let mut bytes = vec![0; 32 * 1024];
    bytes[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    bytes[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    bytes[0x1f0] = region;
    if save {
        bytes[0x1b0..0x1bc]
            .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 0x3f, 0xff]);
    }
    for (index, word) in words.iter().enumerate() {
        bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    bytes
}

fn machine(state: &CoreState) -> &zeff_md_core::Emulator {
    let ActiveCore::Md(emu) = &state.core else {
        panic!("expected MD core");
    };
    emu
}

fn write_word(words: &mut Vec<u16>, address: u32, value: u16) {
    words.extend([0x33fc, value, (address >> 16) as u16, address as u16]);
}

fn write_byte(words: &mut Vec<u16>, address: u32, value: u8) {
    words.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

pub(super) fn guest() -> Vec<u8> {
    let mut words = Vec::new();
    write_byte(&mut words, 0xff0000, 0x5a);
    words.extend([0x13f9, 0x00a1, 0x0003, 0x00ff, 0x0002]);
    words.extend([0x13f9, 0x00a1, 0x0005, 0x00ff, 0x0003]);
    write_byte(&mut words, 0x200001, 0xa5);
    for value in [0x80, 0x04, 0x90] {
        write_byte(&mut words, 0xc00011, value);
    }
    write_word(&mut words, 0xc00004, 0x8f02);
    for (command, value) in [
        (0x40000000_u32, 0x1234),
        (0xc0000000, 0x0eee),
        (0x40000010, 0x0235),
    ] {
        write_word(&mut words, 0xc00004, (command >> 16) as u16);
        write_word(&mut words, 0xc00004, command as u16);
        write_word(&mut words, 0xc00000, value);
    }
    words.extend([0x60fe, 0x4e71]);
    rom(b'U', &words, true)
}

#[test]
fn md_loads_only_supported_raw_formats_and_uses_header_region() {
    for (region, expected) in [
        (b'U', Region::NtscUs),
        (b'E', Region::Pal),
        (b'J', Region::NtscJapan),
    ] {
        for extension in ["md", "gen", "MD", "GEN"] {
            let state = CoreState::from_rom(
                &rom(region, &[0x60fe, 0x4e71], false),
                &format!("load.{extension}"),
            )
            .unwrap();
            assert_eq!(machine(&state).region(), expected);
            assert_eq!(machine(&state).sample_rate(), state.sample_rate);
            assert_eq!(state.sram_size(), 0);
            assert!(state.battery_sram().is_none());
            assert_eq!(state.is_pal_region(), expected == Region::Pal);
            assert_eq!(
                state.fps(),
                1_000_000_000.0 / machine(&state).nominal_frame_duration_ns() as f64
            );
            assert_eq!(
                state.fixed_serialize_size(),
                Some(zeff_md_core::MAX_FRONTEND_STATE_BYTES)
            );
        }
    }
    let extensions = crate::callbacks::VALID_EXTENSIONS.to_str().unwrap();
    for extension in ["smd", "32x"] {
        assert!(!extensions.split('|').any(|item| item == extension));
        assert!(
            CoreState::from_rom(
                &rom(b'U', &[0x60fe, 0x4e71], false),
                &format!("reject.{extension}")
            )
            .is_err()
        );
    }
    let mut expansion = rom(b'U', &[0x60fe, 0x4e71], false);
    for console in [b"SEGA 32X        ", b"SEGA CD         "] {
        expansion[0x100..0x110].copy_from_slice(console);
        assert!(CoreState::from_rom(&expansion, "expansion.md").is_err());
    }
    assert!(CoreState::from_rom(&[0; 7], "invalid.md").is_err());
    let mut unsupported_save = rom(b'U', &[0x60fe, 0x4e71], true);
    unsupported_save[0x1b2] = 0;
    assert!(CoreState::from_rom(&unsupported_save, "unsupported.gen").is_err());
}

#[test]
fn md_guest_executes_ram_pad_save_vdp_and_audio_through_libretro_core_state() {
    let mut state = CoreState::from_rom(&guest(), "guest.md").unwrap();
    state.set_input(0x01, 0x04);
    state.set_input_p2(0x02, 0x01);
    state.step_frame();
    assert_eq!(state.take_runtime_fault(), None);
    assert_eq!(machine(&state).pacing_field_count(), 1);
    assert_eq!(machine(&state).work_ram()[0], 0x5a);
    assert_eq!(machine(&state).work_ram()[2], 0x6e);
    assert_eq!(machine(&state).work_ram()[3], 0x57);
    assert_eq!(state.battery_sram().unwrap()[0], 0xa5);
    state.refresh_system_ram();
    state.refresh_video_ram();
    assert_eq!(state.system_ram_buf, machine(&state).work_ram());
    assert_eq!(&state.video_ram_buf[..2], &[0x12, 0x34]);
    let mut copied = Vec::new();
    for (id, expected) in [("cram", [0x0e, 0xee]), ("vsram", [0x02, 0x35])] {
        state.copy_memory_region(id, &mut copied).unwrap();
        assert_eq!(&copied[..2], &expected);
    }
    let regions = state.memory_regions();
    let cpu = regions.iter().find(|region| region.id == "cpu").unwrap();
    assert_eq!(cpu.address_bits, Some(24));
    assert!(!cpu.writable && !cpu.copyable);
    assert_eq!(cpu.size, None);
    assert_eq!(
        state.memory_region_size(MemoryRegionKind::VideoScrollRam),
        80
    );
    assert_eq!(state.palette_ram_size(), 128);
    assert_copyable_regions(&mut state);
    state.drain_audio();
    assert!(!state.audio_buf.is_empty() && state.audio_buf.len().is_multiple_of(2));
    assert!(state.audio_buf.iter().all(|sample| sample.is_finite()));
    assert!(state.audio_buf.iter().any(|sample| *sample != 0.0));
    state.drain_audio();
    assert!(state.audio_buf.is_empty());
}

#[test]
fn md_state_restore_continues_exactly_and_failed_restore_preserves_machine() {
    let bytes = guest();
    let mut state = CoreState::from_rom(&bytes, "state.md").unwrap();
    state.set_port_device(0, crate::input::MD_SIX_BUTTON_DEVICE);
    state.step_frame();
    state.drain_audio();
    let saved = state.encode_state().unwrap();
    let mut control = CoreState::from_rom(&bytes, "state.gen").unwrap();
    control.set_port_device(0, crate::input::MD_SIX_BUTTON_DEVICE);
    control.load_state(&saved).unwrap();
    state.step_frame();
    state.load_state(&saved).unwrap();
    assert_eq!(
        state.encode_state().unwrap(),
        control.encode_state().unwrap()
    );
    let mut invalid = saved.clone();
    invalid[0] ^= 1;
    for rejected in [&invalid[..], &saved[..saved.len() - 1]] {
        assert!(state.load_state(rejected).is_err());
        assert_eq!(state.encode_state().unwrap(), saved);
    }
    let mut different_rom = bytes;
    different_rom[0x3000] ^= 1;
    let wrong = CoreState::from_rom(&different_rom, "other.md")
        .unwrap()
        .encode_state()
        .unwrap();
    assert!(state.load_state(&wrong).is_err());
    assert_eq!(state.encode_state().unwrap(), saved);
    state.step_frame();
    control.step_frame();
    assert_eq!(
        state.encode_state().unwrap(),
        control.encode_state().unwrap()
    );
    state.drain_audio();
    control.drain_audio();
    assert_eq!(state.audio_buf, control.audio_buf);
}

#[test]
fn md_reset_uses_hardware_reset_and_preserves_save_rate_and_controller_kind() {
    let mut state = CoreState::from_rom(&guest(), "reset.md").unwrap();
    state.set_port_device(0, crate::input::MD_SIX_BUTTON_DEVICE);
    state.set_input(0x04, 0);
    state.step_frame();
    let save = state.battery_sram().unwrap();
    let mut expected = machine(&state).clone();
    zeff_emu_common::time::Reset::reset(&mut expected);
    state.reset().unwrap();
    assert_eq!(machine(&state), &expected);
    assert_eq!(state.battery_sram().unwrap(), save);
    assert_eq!(machine(&state).sample_rate(), state.sample_rate);
    assert_eq!(
        machine(&state).input().controller_kind(ControllerPort::One),
        ControllerKind::SixButton
    );
    assert!(machine(&state).work_ram().iter().all(|byte| *byte == 0));
}

#[test]
fn md_selected_six_button_pad_exposes_polled_extras_through_guest_th_handshake() {
    let mut words = Vec::new();
    for (data, control, destination) in [
        (0xa10003, 0xa10009, 0xff0000),
        (0xa10005, 0xa1000b, 0xff0001),
    ] {
        write_byte(&mut words, control, 0x40);
        for value in [0, 0x40, 0, 0x40, 0, 0x40] {
            write_byte(&mut words, data, value);
        }
        words.extend([
            0x13f9,
            (data >> 16) as u16,
            data as u16,
            (destination >> 16) as u16,
            destination as u16,
        ]);
    }
    words.extend([0x60fe, 0x4e71]);
    let bytes = rom(b'U', &words, false);
    for device in [
        crate::api::RETRO_DEVICE_JOYPAD,
        crate::input::MD_SIX_BUTTON_DEVICE,
    ] {
        for (button, extra) in [(0x10, 0x0b), (0x80, 0x0d), (0x20, 0x0e), (0x04, 0x07)] {
            let mut state = CoreState::from_rom(&bytes, "handshake.md").unwrap();
            for port in 0..2 {
                state.set_port_device(port, device);
            }
            state.set_input(button, 0);
            state.set_input_p2(button, 0);
            state.step_frame();
            assert_eq!(state.take_runtime_fault(), None);
            let expected = if device == crate::input::MD_SIX_BUTTON_DEVICE {
                0x70 | extra
            } else {
                0x7f
            };
            assert_eq!(&machine(&state).work_ram()[..2], &[expected, expected]);
        }
    }
}

#[test]
fn md_restore_keeps_same_rate_audio_state_and_applies_host_rate_only_on_mismatch() {
    let mut state = CoreState::from_rom(&guest(), "rate.md").unwrap();
    state.step_frame();
    let saved = state.encode_state().unwrap();
    state.load_state(&saved).unwrap();
    assert_eq!(state.encode_state().unwrap(), saved);
    let ActiveCore::Md(emu) = &mut state.core else {
        unreachable!();
    };
    emu.set_sample_rate(96_000);
    let foreign_rate = emu.encode_frontend_state().unwrap();
    state.load_state(&foreign_rate).unwrap();
    assert_eq!(machine(&state).sample_rate(), state.sample_rate);
    state.step_frame();
    state.drain_audio();
    assert!(!state.audio_buf.is_empty());
    assert!(state.audio_buf.iter().all(|sample| sample.is_finite()));
}

pub(super) fn interlace_rom() -> Vec<u8> {
    rom(
        b'E',
        &[
            0x33fc, 0x810c, 0x00c0, 0x0004, 0x33fc, 0x8c87, 0x00c0, 0x0004, 0x60fe, 0x4e71,
        ],
        false,
    )
}

#[test]
fn md_geometry_tracks_mode2_and_reset_restore_with_exact_video_conversion() {
    unknown_cram_duplicates_video();
    let mut state = CoreState::from_rom(&interlace_rom(), "video.gen").unwrap();
    assert_eq!(
        (
            state.video_geometry().base_width,
            state.video_geometry().base_height
        ),
        (256, 224)
    );
    assert_eq!(
        (
            state.video_geometry().max_width,
            state.video_geometry().max_height
        ),
        (320, 480)
    );
    assert!(state.take_md_av_change().is_none());
    for _ in 0..3 {
        state.step_frame();
    }
    assert_eq!(state.take_runtime_fault(), None);
    let av = state.take_md_av_change().unwrap();
    assert_eq!(
        (av.geometry.base_width, av.geometry.base_height),
        (320, 480)
    );
    assert_eq!(av.timing.fps, state.fps());
    let rgba = machine(&state).framebuffer().to_vec();
    let xrgb = state.framebuffer_as_xrgb8888().to_vec();
    let rgb565 = state.framebuffer_as_rgb565().to_vec();
    assert_eq!(xrgb.len(), 320 * 480 * 4);
    assert_eq!(rgb565.len(), 320 * 480 * 2);
    for (index, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
        assert_eq!(
            &xrgb[index * 4..index * 4 + 4],
            &[pixel[2], pixel[1], pixel[0], 0]
        );
        let packed = ((u16::from(pixel[0]) >> 3) << 11)
            | ((u16::from(pixel[1]) >> 2) << 5)
            | (u16::from(pixel[2]) >> 3);
        assert_eq!(&rgb565[index * 2..index * 2 + 2], &packed.to_le_bytes());
    }
    let saved = state.encode_state().unwrap();
    state.reset().unwrap();
    assert_eq!(state.take_md_av_change().unwrap().geometry.base_height, 224);
    state.load_state(&saved).unwrap();
    assert_eq!(state.take_md_av_change().unwrap().geometry.base_height, 480);
}

fn unknown_cram_duplicates_video() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DUPLICATES: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn duplicate(
        data: *const std::ffi::c_void,
        width: u32,
        height: u32,
        pitch: usize,
    ) {
        if data.is_null() && (width, height, pitch) == (256, 224, 0) {
            DUPLICATES.fetch_add(1, Ordering::Relaxed);
        }
    }
    DUPLICATES.store(0, Ordering::Relaxed);
    let mut words = Vec::new();
    for value in [0x8114, 0x8f02, 0x9301, 0x9780, 0xc07e, 0x0080] {
        write_word(&mut words, 0xc00004, value);
    }
    write_word(&mut words, 0xc00000, 0xabcd);
    words.extend([0x60fe, 0x4e71]);
    let mut state = CoreState::from_rom(&rom(b'U', &words, false), "cram.md").unwrap();
    for _ in 0..2 {
        state.step_frame();
        assert!(state.take_runtime_fault().is_none());
        assert!(!state.framebuffer_available());
        assert_eq!(machine(&state).framebuffer().len(), 256 * 224 * 4);
        state.present_video(duplicate, Some(0), false);
        state.present_video(duplicate, Some(0), true);
        state.drain_audio();
        assert!(!state.audio_buf.is_empty());
    }
    assert_eq!(DUPLICATES.load(Ordering::Relaxed), 0);
    let mut output = vec![0x5a];
    for region in ["palette_ram", "framebuffer"] {
        assert!(state.copy_memory_region(region, &mut output).is_err());
        assert_eq!(output, [0x5a]);
    }
}

#[test]
fn md_rejected_cheats_do_not_mutate_machine_or_install_patches() {
    let mut state = CoreState::from_rom(&guest(), "cheats.md").unwrap();
    let before = state.encode_state().unwrap();
    for code in [
        "000100:99",
        "A10003:00",
        "E00000:99",
        "01FF0000:99",
        "FF0000:99+nonsense",
    ] {
        state.set_indexed_cheat(0, Some(code));
    }
    state.apply_ram_cheats();
    state.cheat_reset();
    assert!(state.ram_cheats.is_empty());
    assert_eq!(state.encode_state().unwrap(), before);
}

#[test]
fn md_runtime_fault_is_reported_without_faking_field_completion() {
    let bytes = rom(b'U', &[0x3039, 0x0040, 0x0000, 0x60fe, 0x4e71], false);
    let mut state = CoreState::from_rom(&bytes, "fault.md").unwrap();
    state.step_frame();
    assert!(
        state
            .take_runtime_fault()
            .unwrap()
            .contains("UnsupportedAccess")
    );
    assert!(state.take_runtime_fault().is_none());
    assert_eq!(machine(&state).pacing_field_count(), 0);
    assert!(state.encode_state().is_err());
    let frozen = machine(&state).clone();
    state.step_frame();
    assert_eq!(machine(&state), &frozen);
    assert!(state.take_runtime_fault().unwrap().contains("Frozen"));
    state.reset().unwrap();
    assert!(state.take_runtime_fault().is_none());
}

#[path = "md/frontend.rs"]
mod frontend;

#[test]
fn md_profile_auto_load_uses_payload_region_and_ntsc_metadata() {
    let mut bytes = rom(b'U', &[0x60fe, 0x4e71], false);
    bytes.resize(0x80000, 0);
    bytes[0x180..0x18e].copy_from_slice(b"GM 00001011-00");
    bytes[0x1f0..0x1f3].copy_from_slice(b"JUE");
    bytes[0x7fffe..].copy_from_slice(&0xa3bf_u16.to_be_bytes());
    assert_eq!(
        zeff_md_core::CartridgeHeader::parse(&bytes)
            .unwrap()
            .calculated_checksum,
        0x532e
    );
    let state = CoreState::from_rom(&bytes, "region-profile.md").unwrap();
    assert_eq!(machine(&state).region(), Region::NtscJapan);
    assert!(!state.is_pal_region());
    assert_eq!(machine(&state).sample_rate(), state.sample_rate);
}
