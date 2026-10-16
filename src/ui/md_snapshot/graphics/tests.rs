use super::super::collect_md_snapshot;
use super::super::memory_tests::{fixture, request};
use super::*;
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::{LoadConfig, Region};

fn graphics(data: ConsoleGraphicsData) -> Box<MdGraphicsData> {
    let ConsoleGraphicsData::Md(gfx) = data else {
        panic!("expected Mega Drive graphics");
    };
    gfx
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_cram_normal_intensity_uses_literal_three_bit_channels() {
    let levels = [0, 36, 72, 109, 145, 182, 218, 255];
    for (index, expected) in levels.into_iter().enumerate() {
        assert_eq!(rgba((index as u16) << 1), [expected, 0, 0, 255]);
        assert_eq!(rgba((index as u16) << 5), [0, expected, 0, 255]);
        assert_eq!(rgba((index as u16) << 9), [0, 0, expected, 255]);
    }
    assert_eq!(rgba(0x0246), [109, 72, 36, 255]);
    assert_eq!(rgba(0x0ace), [255, 218, 182, 255]);
    assert_eq!(rgba(0xf111), [0, 0, 0, 255]);
    assert_eq!(rgba(0xffff), [255; 4]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_pattern_geometry_requires_known_supported_mode_bits() {
    for size in [0, 0x81, 0x08, 0x89] {
        assert_eq!(tile_height(Some(4), Some(size)), Some(8));
        assert_eq!(tile_height(Some(0x44), Some(size | 6)), Some(16));
    }
    for (mode, size) in [
        (None, Some(0)),
        (Some(4), None),
        (Some(0), Some(0)),
        (Some(0x84), Some(0)),
        (Some(4), Some(2)),
        (Some(4), Some(4)),
    ] {
        assert_eq!(tile_height(mode, size), None);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_graphics_snapshot_obeys_existing_viewer_gates_and_palette_rows() {
    let emu = fixture(Region::NtscUs, false);
    let mut req = request();
    req.show_memory_viewer = false;
    let closed = collect_md_snapshot(&emu, &req, None);
    assert!(closed.palette_debug.is_none() && closed.graphics_data.is_none());
    req.any_viewer_open = true;
    let palette_only = collect_md_snapshot(&emu, &req, None);
    assert!(palette_only.graphics_data.is_none());
    let palette = palette_only.palette_debug.unwrap();
    assert_eq!(palette.groups.len(), 1);
    assert_eq!(palette.groups[0].rows.len(), 4);
    for (index, row) in palette.groups[0].rows.iter().enumerate() {
        assert_eq!(row.label, format!("Palette {index}"));
        assert_eq!(row.colors.len(), 16);
    }
    assert_eq!(palette.groups[0].rows[0].colors[0], [0, 0, 255, 255]);
    assert_eq!(palette.groups[0].rows[0].colors[1], [255, 0, 0, 255]);
    req.any_vram_viewer_open = true;
    let open = collect_md_snapshot(&emu, &req, None);
    assert!(open.oam_debug.is_none() && open.disassembly_view.is_none());
    let gfx = graphics(open.graphics_data.unwrap());
    assert_eq!(gfx.vram.len(), 0x10000);
    assert_eq!(&gfx.vram[..32], &[0x11; 32]);
    assert_eq!(gfx.tile_height, Some(8));
    for (row, colors) in palette.groups[0]
        .rows
        .iter()
        .zip(gfx.palette.as_chunks::<16>().0.iter())
    {
        assert_eq!(row.colors, colors);
    }
    req.any_viewer_open = false;
    assert!(
        collect_md_snapshot(&emu, &req, None)
            .palette_debug
            .is_none()
    );
    req.any_vram_viewer_open = false;
    assert!(
        collect_md_snapshot(&emu, &req, None)
            .graphics_data
            .is_none()
    );
}

fn changing_vdp(mode2: bool) -> Emulator {
    let mut rom = vec![0; 0x1000];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    let mut words = Vec::<u16>::new();
    for value in [0x8104, 0x8c00 | if mode2 { 6 } else { 0 }, 0x8f02] {
        words.extend([0x33fc, value, 0x00c0, 4]);
    }
    for (pattern, color) in [(0x1234, 0x0246), (0x5678, 0x0ace)] {
        for (port, value) in [
            (4, 0x4000),
            (4, 0),
            (0, pattern),
            (4, 0x4000),
            (4, 0x10),
            (0, if pattern == 0x1234 { 0xfa35 } else { 0xf6ab }),
            (4, 0x404e),
            (4, 0x10),
            (0, if pattern == 0x1234 { 0xffff } else { 0xf012 }),
            (4, 0xc07e),
            (4, 0),
            (0, if pattern == 0x1234 { 0xffff } else { 0xf246 }),
            (4, 0xc000),
            (4, 0),
            (0, color),
        ] {
            words.extend([0x33fc, value, 0x00c0, port]);
        }
        words.extend([0x4e71; 64]);
    }
    words.push(0x60fe);
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(rom, LoadConfig::default()).unwrap()
}

fn wait_color(emu: &mut Emulator, color: u16) {
    for _ in 0..4096 {
        if emu.vdp().memory().cram()[0] == color {
            return;
        }
        emu.step_instruction().unwrap();
    }
    panic!("VDP fixture did not publish {color:04X}");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_graphics_snapshots_refresh_and_restore_detached_vram_cram_and_mode2() {
    for mode2 in [false, true] {
        let mut emu = changing_vdp(mode2);
        wait_color(&mut emu, 0x0246);
        let original = emu.encode_state().unwrap();
        let mut req = request();
        req.show_memory_viewer = false;
        req.any_viewer_open = true;
        req.any_vram_viewer_open = true;
        let before = graphics(collect_md_snapshot(&emu, &req, None).graphics_data.unwrap());
        assert_eq!(&before.vram[..2], &[0x12, 0x34]);
        assert_eq!(before.palette[0], [109, 72, 36, 255]);
        assert_eq!(before.cram.len(), 128);
        assert_eq!(before.vsram.len(), 80);
        assert_eq!(&before.cram[..2], &[0x02, 0x46]);
        assert_eq!(&before.cram[126..], &[0x0e, 0xee]);
        assert_eq!(&before.vsram[..2], &[0x02, 0x35]);
        assert_eq!(&before.vsram[78..], &[0x07, 0xff]);
        assert_eq!(before.tile_height, Some(if mode2 { 16 } else { 8 }));
        wait_color(&mut emu, 0x0ace);
        let after = collect_md_snapshot(&emu, &req, None);
        let updated = graphics(after.graphics_data.unwrap());
        assert_eq!(&updated.vram[..2], &[0x56, 0x78]);
        assert_eq!(updated.palette[0], [255, 218, 182, 255]);
        assert_eq!(&updated.cram[..2], &[0x0a, 0xce]);
        assert_eq!(&updated.cram[126..], &[0x02, 0x46]);
        assert_eq!(&updated.vsram[..2], &[0x06, 0xab]);
        assert_eq!(&updated.vsram[78..], &[0x00, 0x12]);
        assert_eq!(&before.cram[..2], &[0x02, 0x46]);
        assert_eq!(&before.vsram[..2], &[0x02, 0x35]);
        assert_eq!(
            after.palette_debug.unwrap().groups[0].rows[0].colors[0],
            updated.palette[0]
        );
        assert_eq!(&before.vram[..2], &[0x12, 0x34]);
        assert_eq!(before.palette[0], [109, 72, 36, 255]);
        emu.decode_state(&original).unwrap();
        let restored = graphics(collect_md_snapshot(&emu, &req, None).graphics_data.unwrap());
        assert_eq!(restored.vram, before.vram);
        assert_eq!(restored.palette, before.palette);
        assert_eq!(restored.cram, before.cram);
        assert_eq!(restored.vsram, before.vsram);
        assert_eq!(restored.tile_height, before.tile_height);
        assert_eq!(emu.encode_state().unwrap(), original);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_graphics_inspection_preserves_machine_and_future_frame_pcm() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            let mut emu = fixture(region, h40);
            let mut control = emu.clone();
            let original = emu.encode_state().unwrap();
            let mut req = request();
            req.show_memory_viewer = false;
            for (palette, tiles) in [(false, false), (true, false), (false, true), (true, true)] {
                req.any_viewer_open = palette;
                req.any_vram_viewer_open = tiles;
                let data = collect_md_snapshot(&emu, &req, None);
                assert_eq!(data.palette_debug.is_some(), palette);
                assert_eq!(data.graphics_data.is_some(), tiles);
                assert_eq!(emu, control);
                assert_eq!(emu.encode_state().unwrap(), original);
            }
            let frame_before = emu.frame_count();
            let ticks = region.frame_ticks();
            let end = (emu.timing_snapshot().now().get() / ticks + 2) * ticks;
            while emu.timing_snapshot().now().get() < end {
                assert_eq!(
                    emu.step_instruction().unwrap(),
                    control.step_instruction().unwrap()
                );
            }
            assert!(emu.frame_count() > frame_before);
            assert_eq!(emu.video_error(), None);
            assert_eq!(
                emu.framebuffer_dimensions(),
                (if h40 { 320 } else { 256 }, 224)
            );
            assert_eq!(
                emu.framebuffer_dimensions(),
                control.framebuffer_dimensions()
            );
            assert!(
                emu.framebuffer()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == [255, 0, 0, 255])
            );
            assert_eq!(emu.framebuffer(), control.framebuffer());
            let actual = emu.drain_audio_samples();
            let expected = control.drain_audio_samples();
            assert!(!actual.is_empty() && actual.iter().any(|sample| *sample != 0.0));
            assert_eq!(actual, expected);
            assert_eq!(emu.encode_state().unwrap(), control.encode_state().unwrap());
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_raw_color_and_scroll_snapshots_cover_every_backing_word_in_bus_byte_order() {
    let mut rom = vec![0; 0x2000];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    let mut words = vec![0x33fc_u16, 0x8104, 0x00c0, 4, 0x33fc, 0x8f02, 0x00c0, 4];
    for (command, raw) in [
        (
            0xc000,
            (0..64)
                .map(|i| 0xfb11 | ((i & 7) << 1) | ((i >> 3) << 5))
                .collect::<Vec<_>>(),
        ),
        (
            0x4000,
            (0..40)
                .map(|i| 0xf800 | (0x123 + i * 37))
                .collect::<Vec<_>>(),
        ),
    ] {
        words.extend([0x33fc, command, 0x00c0, 4]);
        words.extend([0x33fc, if command == 0xc000 { 0 } else { 0x10 }, 0x00c0, 4]);
        for value in raw {
            words.extend([0x33fc, value, 0x00c0, 0]);
        }
    }
    words.push(0x60fe);
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut emu = Emulator::new(rom, LoadConfig::default()).unwrap();
    for _ in 0..512 {
        emu.step_instruction().unwrap();
    }
    let original = emu.encode_state().unwrap();
    let gfx = graphics(snapshot(&emu));
    assert_eq!(gfx.cram.len(), 128);
    assert_eq!(gfx.vsram.len(), 80);
    for (i, bytes) in gfx.cram.as_chunks::<2>().0.iter().enumerate() {
        assert_eq!(bytes, &[0x0a, (((i & 7) << 1) | ((i >> 3) << 5)) as u8]);
    }
    for (i, bytes) in gfx.vsram.as_chunks::<2>().0.iter().enumerate() {
        let expected = 0x123 + i * 37;
        assert_eq!(bytes, &[(expected >> 8) as u8, expected as u8]);
    }
    assert_eq!(&gfx.cram[..2], &[0x0a, 0]);
    assert_eq!(&gfx.cram[126..], &[0x0a, 0xee]);
    assert_eq!(&gfx.vsram[..2], &[0x01, 0x23]);
    assert_eq!(&gfx.vsram[78..], &[0x06, 0xc6]);
    assert_eq!(emu.encode_state().unwrap(), original);
}
