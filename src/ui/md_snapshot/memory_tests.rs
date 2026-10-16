use super::*;
use crate::emu_thread::{MemorySearchRequest, RenderSettings};
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::LoadConfig;

pub(super) fn fixture(region: Region, h40: bool) -> Emulator {
    let mut rom = vec![0; 0x20000];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    rom[0x10010..0x10014].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    rom[0xffff..0x10003].copy_from_slice(&[0x35, 0x79, 0x24, 0x68]);
    rom[0x1fffe..].copy_from_slice(&[0xab, 0xcd]);
    let mut words = Vec::new();
    for (index, value) in [
        (1, 4),
        (2, 0x30),
        (3, 0x2c),
        (4, 5),
        (5, 0x10),
        (7, 0),
        (11, 0),
        (12, if h40 { 0x81 } else { 0 }),
        (13, 0x3f),
        (15, 2),
        (16, 1),
        (17, 0),
        (18, 0),
    ] {
        words.extend([0x33fc, 0x8000 | (index << 8) | value, 0x00c0, 4]);
    }
    for (port, value) in [
        (4, 0xc000),
        (4, 0),
        (0, 0x0e00),
        (0, 0x000e),
        (4, 0x4000),
        (4, 0),
    ] {
        words.extend([0x33fc, value, 0x00c0, port]);
    }
    for _ in 0..16 {
        words.extend([0x33fc, 0x1111, 0x00c0, 0]);
    }
    words.extend([0x33fc, 0x8144, 0x00c0, 4]);
    for (address, value) in [
        (0xff0000_u32, 0xef),
        (0xff0001, 0x98),
        (0xff0040, 0xde),
        (0xff0041, 0xad),
        (0xff0042, 0xbe),
        (0xff0043, 0xef),
        (0xfffffe, 0x12),
        (0xffffff, 0x34),
        (0xc00011, 0x80),
        (0xc00011, 0x02),
        (0xc00011, 0x90),
    ] {
        words.extend([0x13fc_u16, value, (address >> 16) as u16, address as u16]);
    }
    words.extend([0x5239, 0x00ff, 0x0010, 0x60f8]);
    for (index, word) in words.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut emu = Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .unwrap();
    for _ in 0..256 {
        emu.step_instruction().unwrap();
    }
    emu
}

pub(super) fn request() -> SnapshotRequest {
    SnapshotRequest {
        want_debug_info: false,
        want_perf_info: false,
        any_viewer_open: false,
        any_vram_viewer_open: false,
        show_oam_viewer: false,
        show_apu_viewer: false,
        show_disassembler: false,
        show_rom_info: false,
        show_memory_viewer: true,
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
            color_correction: crate::settings::ColorCorrection::None,
            color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            dmg_palette_preset: Default::default(),
            nes_palette_mode: Default::default(),
            nes_custom_palette: None,
            pce_overscan_mode: Default::default(),
            pce_palette_mode: Default::default(),
            sgb_border_enabled: false,
        },
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_memory_pages_preserve_physical_addresses_and_reuse_storage() {
    let emu = fixture(Region::NtscUs, false);
    let mut req = request();
    let mut reusable = Some(Vec::with_capacity(256));
    let pointer = reusable.as_ref().unwrap().as_ptr();
    for start in [
        0, 0xffff, 0x10000, 0x10010, 0xe00000, 0xff0000, 0xffff00, 0xfffffe, 0xffffff,
    ] {
        req.memory_view_start = start;
        let data = collect_md_snapshot(&emu, &req, reusable);
        let page = data.memory_page.unwrap();
        assert_eq!(page.as_ptr(), pointer);
        assert_eq!(page.first().unwrap().0, start);
        assert_eq!(page.len(), ((0x1000000 - start) as usize).min(256));
        for (index, &(address, byte)) in page.iter().enumerate() {
            assert_eq!(address, start + index as u32);
            assert_eq!(byte, emu.peek(address).unwrap_or(0xff));
        }
        reusable = Some(page);
    }
    req.memory_view_start = 0xfffffe;
    assert_eq!(
        collect_md_snapshot(&emu, &req, None).memory_page.unwrap(),
        [(0xfffffe, 0x12), (0xffffff, 0x34)]
    );
    req.memory_view_start = 0x1000000;
    assert!(
        collect_md_snapshot(&emu, &req, None)
            .memory_page
            .unwrap()
            .is_empty()
    );
    req.memory_view_start = 0xc00000;
    assert_eq!(
        collect_md_snapshot(&emu, &req, None).memory_page.unwrap()[0],
        (0xc00000, 0xff)
    );
    req.show_memory_viewer = false;
    let closed = collect_md_snapshot(&emu, &req, reusable);
    let page = closed.memory_page.unwrap();
    assert!(page.is_empty());
    assert_eq!(page.as_ptr(), pointer);
    assert!(closed.memory_search_results.is_none());
    assert!(collect_md_snapshot(&emu, &req, None).memory_page.is_none());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_memory_search_is_bounded_to_loaded_rom_and_canonical_work_ram() {
    let emu = fixture(Region::NtscUs, false);
    let mut req = request();
    req.show_memory_viewer = false;
    for (pattern, limit, expected) in [
        (vec![0xde, 0xad, 0xbe, 0xef], 8, vec![0x10010, 0xff0040]),
        (vec![0xde, 0xad, 0xbe, 0xef], 1, vec![0x10010]),
        (vec![0xde, 0xad, 0xbe, 0xef], 0, vec![]),
        (vec![0x35, 0x79, 0x24, 0x68], 8, vec![0xffff]),
        (vec![0x12, 0x34], 8, vec![0xfffffe]),
        (vec![0xab, 0xcd, 0xef, 0x98], 8, vec![]),
        (vec![], 8, vec![]),
        (vec![0; 0x20001], 8, vec![]),
    ] {
        req.memory_search = Some(MemorySearchRequest {
            pattern: pattern.clone(),
            max_results: limit,
        });
        let results = collect_md_snapshot(&emu, &req, None)
            .memory_search_results
            .unwrap();
        assert_eq!(
            results.iter().map(|item| item.address).collect::<Vec<_>>(),
            expected
        );
        assert!(results.iter().all(|item| item.matched_bytes == pattern));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_memory_inspection_preserves_machine_state_and_future_frame_pcm() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            let mut emu = fixture(region, h40);
            let mut control = emu.clone();
            let state = emu.encode_state().unwrap();
            let mut req = request();
            req.memory_search = Some(MemorySearchRequest {
                pattern: vec![0xde, 0xad, 0xbe, 0xef],
                max_results: 8,
            });
            let mut reusable = None;
            for start in [
                0, 0x10010, 0xa00000, 0xa04000, 0xa10000, 0xc00000, 0xff0000, 0xfffffe,
            ] {
                req.memory_view_start = start;
                reusable = collect_md_snapshot(&emu, &req, reusable).memory_page;
                assert_eq!(emu, control);
                assert_eq!(emu.encode_state().unwrap(), state);
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
            assert_eq!(emu, control);
            assert!(emu.frame_count() > frame_before);
            assert_eq!(emu.video_error(), None);
            assert_eq!(
                emu.framebuffer_dimensions(),
                (if h40 { 320 } else { 256 }, 224)
            );
            assert!(
                emu.framebuffer()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == [255, 0, 0, 255])
            );
            assert_eq!(
                emu.framebuffer_dimensions(),
                control.framebuffer_dimensions()
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
