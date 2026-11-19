use super::*;
use zeff_emu_common::memory::{MemoryRegionKind, MemoryRegionView, resolve_memory_region};

fn memory_guest(header: u8, save_size: usize) -> Vec<u8> {
    let mut words = Vec::new();
    for (address, value) in [
        (0xff0000, 0x91),
        (0xff0001, 0x73),
        (0xfffffe, 0x25),
        (0xffffff, 0xe6),
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
    for (port, value) in [
        (4, 0x8f02),
        (4, 0x8c81),
        (4, if header == b'E' { 0x810c } else { 0x8104 }),
        (4, 0x4000),
        (4, 0),
        (0, 0xabcd),
        (4, 0x7ffe),
        (4, 3),
        (0, 0x1357),
        (4, 0xc000),
        (4, 0),
        (0, 0xf246),
        (4, 0xc07e),
        (4, 0),
        (0, 0xffff),
        (4, 0x4000),
        (4, 0x10),
        (0, 0xfa35),
        (4, 0x404e),
        (4, 0x10),
        (0, 0xffab),
    ] {
        write_word(&mut words, 0xc00000 + port, value);
    }
    if save_size != 0 {
        write_byte(&mut words, 0xa130f1, 1);
        write_byte(&mut words, 0x200001, 0x65);
        write_byte(&mut words, 0x200001 + (save_size as u32 - 1) * 2, 0xa9);
    }
    words.extend(LOOP);
    let mut bytes = rom(header, &words);
    if save_size != 0 {
        bytes[0x1b0..0x1b4].copy_from_slice(&[b'R', b'A', 0xf8, 0x20]);
        bytes[0x1b4..0x1b8].copy_from_slice(&0x200001_u32.to_be_bytes());
        let end = 0x200001 + (save_size as u32 - 1) * 2;
        bytes[0x1b8..0x1bc].copy_from_slice(&end.to_be_bytes());
    }
    bytes
}

fn fixture(header: u8, save_size: usize) -> EmuBackend {
    let mut backend = load(
        memory_guest(header, save_size),
        BackendLoadConfig {
            md_load_save_ram: false,
            sample_rate: Some(48_000),
            ..BackendLoadConfig::default()
        },
    );
    backend.step_frame();
    assert!(!backend.is_suspended());
    assert_eq!(machine(&backend).video_error(), None);
    backend
}

fn exported(backend: &mut EmuBackend, name: &str) -> Vec<u8> {
    let mut output = vec![0x19; 17];
    let descriptor = backend.copy_memory_region(name, &mut output).unwrap();
    assert_eq!(descriptor.size, Some(output.len()));
    assert!(descriptor.readable && descriptor.side_effect_free && descriptor.copyable);
    assert!(!descriptor.writable);
    assert_eq!(descriptor.address_bits, None);
    output
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_memory_descriptors_report_finite_backing_regions_and_read_only_cpu() {
    let backend = fixture(b'U', 512);
    let caps = backend.capabilities();
    assert_eq!(caps.system_ram_len, 0x10000);
    assert_eq!(caps.video_ram_len, 0x10000);
    assert_eq!(caps.save_ram_kind.size(), 512);
    let regions = &caps.memory_regions;
    assert_eq!(regions.len(), 7);
    for (name, kind, size) in [
        ("wram", MemoryRegionKind::SystemRam, 0x10000),
        ("vram", MemoryRegionKind::VideoRam, 0x10000),
        ("cram", MemoryRegionKind::PaletteRam, 128),
        ("vsram", MemoryRegionKind::VideoScrollRam, 80),
        ("sram", MemoryRegionKind::SaveRam, 512),
    ] {
        let descriptor = resolve_memory_region(regions, name).unwrap();
        assert_eq!(descriptor.kind, kind);
        assert_eq!(descriptor.size, Some(size));
        assert_eq!(descriptor.view, MemoryRegionView::Physical);
        assert!(!descriptor.writable);
        assert!(descriptor.readable && descriptor.copyable && descriptor.side_effect_free);
    }
    let cpu = resolve_memory_region(regions, "ram").unwrap();
    assert_eq!(cpu.kind, MemoryRegionKind::CpuAddressSpace);
    assert_eq!(cpu.size, None);
    assert_eq!(cpu.address_bits, Some(24));
    assert_eq!(cpu.view, MemoryRegionView::AddressSpace);
    assert!(cpu.readable && cpu.side_effect_free);
    assert!(!cpu.copyable && !cpu.writable);
    let frame = resolve_memory_region(regions, "fb").unwrap();
    assert_eq!(frame.size, Some(backend.framebuffer().len()));
    assert_eq!(frame.view, MemoryRegionView::Derived);
    assert!(caps.supports_debugger);
    assert!(caps.supports_execution_controls);
    assert!(caps.supports_guest_calls && caps.supports_opcode_history);
    assert!(caps.supports_rewind);
    assert!(caps.supports_cheats && caps.supports_replay);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_memory_copies_preserve_canonical_bytes_aliases_and_backing_bounds() {
    for header in *b"UJE" {
        let mut backend = fixture(header, 8192);
        let ram = exported(&mut backend, "system_ram");
        assert_eq!(ram.len(), 65536);
        assert_eq!(&ram[..2], &[0x91, 0x73]);
        assert_eq!(&ram[65534..], &[0x25, 0xe6]);
        assert_eq!(machine(&backend).work_ram().get(65536), None);
        for alias in ["wram", "system.ram", "SYSTEM-RAM"] {
            assert_eq!(exported(&mut backend, alias), ram);
        }
        for address in [0xe00000, 0xff0000] {
            assert_eq!(machine(&backend).peek(address), Some(0x91));
        }
        let vram = exported(&mut backend, "vram");
        assert_eq!(vram.len(), 65536);
        assert_eq!(&vram[..2], &[0xab, 0xcd]);
        assert_eq!(&vram[65534..], &[0x13, 0x57]);
        for (name, size, first, last, aliases) in [
            (
                "cram",
                128,
                [0x02, 0x46],
                [0x0e, 0xee],
                ["palette", "palette_ram"],
            ),
            (
                "vsram",
                80,
                [0x02, 0x35],
                [0x07, 0xab],
                ["video_scroll_ram", "scrollram"],
            ),
        ] {
            let actual = exported(&mut backend, name);
            assert_eq!(actual.len(), size);
            assert_eq!(&actual[..2], &first);
            assert_eq!(&actual[size - 2..], &last);
            assert_eq!(actual.get(size), None);
            for alias in aliases {
                assert_eq!(exported(&mut backend, alias), actual);
            }
        }
        let save = exported(&mut backend, "battery");
        assert_eq!(save.len(), 8192);
        assert_eq!(save[0], 0x65);
        assert_eq!(save[8191], 0xa9);
        assert_eq!(save.get(8192), None);
        let expected_frame = backend.framebuffer().to_vec();
        assert_eq!(exported(&mut backend, "framebuffer"), expected_frame);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_memory_rejected_exports_leave_output_and_machine_unchanged() {
    let mut backend = fixture(b'U', 0);
    let original = machine(&backend).clone();
    let saved = backend.encode_state_bytes().unwrap();
    assert!(resolve_memory_region(&backend.memory_regions(), "save_ram").is_none());
    for name in [
        "cpu", "ram", "memory", "sram", "battery", "oam", "io", "missing", "",
    ] {
        let mut output = vec![0x31, 0xa5, 0x9c];
        let pointer = output.as_ptr();
        let capacity = output.capacity();
        assert!(
            backend.copy_memory_region(name, &mut output).is_err(),
            "{name}"
        );
        assert_eq!(output, [0x31, 0xa5, 0x9c]);
        assert_eq!(output.as_ptr(), pointer);
        assert_eq!(output.capacity(), capacity);
        assert_eq!(machine(&backend), &original);
        assert_eq!(backend.encode_state_bytes().unwrap(), saved);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_memory_exports_follow_save_capacities_and_restored_state_without_future_effects() {
    for header in *b"UJE" {
        for size in [0, 512, 8192, 32768] {
            let mut inspected = fixture(header, size);
            let original = machine(&inspected).clone();
            let saved = inspected.encode_state_bytes().unwrap();
            for region in inspected
                .memory_regions()
                .into_iter()
                .filter(|r| r.copyable)
            {
                let copied = exported(&mut inspected, region.id);
                assert_eq!(region.size, Some(copied.len()));
                assert_eq!(machine(&inspected), &original);
                assert_eq!(inspected.encode_state_bytes().unwrap(), saved);
            }
            let mut control = load(
                memory_guest(header, size),
                BackendLoadConfig {
                    md_load_save_ram: false,
                    sample_rate: Some(48_000),
                    ..BackendLoadConfig::default()
                },
            );
            control.load_state_from_bytes(saved.clone()).unwrap();
            inspected.load_state_from_bytes(saved.clone()).unwrap();
            for name in ["system_ram", "vram", "cram", "vsram", "framebuffer"] {
                assert_eq!(exported(&mut inspected, name), exported(&mut control, name));
            }
            if size != 0 {
                assert_eq!(exported(&mut inspected, "sram").len(), size);
            }
            assert_eq!(inspected.encode_state_bytes().unwrap(), saved);
            for _ in 0..2 {
                inspected.step_frame();
                control.step_frame();
                assert!(!inspected.is_suspended() && !control.is_suspended());
                assert_eq!(machine(&inspected), machine(&control));
                assert_eq!(inspected.framebuffer(), control.framebuffer());
                let mut actual = Vec::new();
                let mut expected = Vec::new();
                inspected.drain_audio_samples_into(&mut actual);
                control.drain_audio_samples_into(&mut expected);
                assert!(!actual.is_empty() && actual.iter().any(|s| *s != 0.0));
                assert!(actual.iter().all(|s| s.is_finite()));
                assert_eq!(actual, expected);
                assert!(
                    actual
                        .iter()
                        .zip(&expected)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
                assert_eq!(
                    inspected.encode_state_bytes().unwrap(),
                    control.encode_state_bytes().unwrap()
                );
            }
        }
    }
}
