use super::super::super::collect_md_snapshot;
use super::super::memory_tests::{fixture, request};
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::{Emulator, LoadConfig, Region};

fn ram_code() -> Emulator {
    let mut rom = vec![0; 0x400];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    let words: [u16; 19] = [
        0x23fc, 0x5239, 0x00ff, 0x00ff, 0x0000, 0x23fc, 0x0009, 0x60f8, 0x00ff, 0x0004, 0x33fc,
        0x7001, 0x00ff, 0x0008, 0x4ef9, 0x00ff, 0x0000, 0x4e71, 0x4e71,
    ];
    for (i, word) in words.into_iter().enumerate() {
        rom[0x200 + i * 2..0x202 + i * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut emu = Emulator::new(rom, LoadConfig::default()).unwrap();
    for _ in 0..4 {
        emu.step_instruction().unwrap();
    }
    assert_eq!(emu.registers().pc, 0xff0000);
    emu
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_disassembly_rebuilds_same_pc_ram_and_restored_code_without_navigation() {
    let mut emu = ram_code();
    let mut req = request();
    req.show_memory_viewer = false;
    assert!(
        collect_md_snapshot(&emu, &req, None)
            .disassembly_view
            .is_none()
    );
    req.show_disassembler = true;
    req.last_disasm_pc = Some(emu.registers().pc);
    req.disasm_target = Some(crate::debug::DisassemblyTarget {
        cpu_address: 0xc00000,
        storage_offset: Some(0),
        bank: Some(0),
        thumb: None,
    });
    let original = emu.encode_state().unwrap();
    for immediate in 1..=3 {
        let data = collect_md_snapshot(&emu, &req, None);
        let view = data.disassembly_view.unwrap();
        assert_eq!(view.pc, 0xff0000);
        assert!(!view.is_navigation_target && !view.is_static_target);
        assert_eq!(view.lines.len(), 26);
        assert_eq!(view.lines[0].address, 0xff0000);
        assert_eq!(view.lines[0].bytes.as_slice(), &[0x52, 0x39, 0, 0xff, 0, 9]);
        assert_eq!(view.lines[1].address, 0xff0006);
        assert_eq!(view.lines[1].control_target, Some(0xff0000));
        assert_eq!(
            view.lines[2].mnemonic.as_str(),
            format!("MOVEQ #{immediate},D0")
        );
        assert!(view.breakpoints.is_empty() && view.one_shot_breakpoints.is_empty());
        assert!(view.lines.iter().all(|l| l.bank.is_none()
            && l.storage_offset.is_none()
            && l.control_target_bank.is_none()
            && l.control_target_storage.is_none()));
        assert!(data.cpu_debug.is_none());
        for _ in 0..2 {
            emu.step_instruction().unwrap();
        }
        assert_eq!(emu.registers().pc, req.last_disasm_pc.unwrap());
    }
    emu.decode_state(&original).unwrap();
    let view = collect_md_snapshot(&emu, &req, None)
        .disassembly_view
        .unwrap();
    assert_eq!(view.lines[2].mnemonic.as_str(), "MOVEQ #1,D0");
    assert_eq!(emu.encode_state().unwrap(), original);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_disassembly_inspection_preserves_pending_machine_and_future_frame_pcm() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            let mut emu = fixture(region, h40);
            let mut control = emu.clone();
            let encoded = emu.encode_state().unwrap();
            let mut req = request();
            req.show_memory_viewer = false;
            req.show_disassembler = true;
            req.last_disasm_pc = Some(emu.registers().pc);
            for _ in 0..8 {
                let view = collect_md_snapshot(&emu, &req, None)
                    .disassembly_view
                    .unwrap();
                assert_eq!(view.lines.len(), 26);
                assert_eq!(view.lines[0].address, emu.registers().pc & 0xffffff);
                assert_eq!(emu, control);
                assert_eq!(emu.encode_state().unwrap(), encoded);
            }
            let frame = emu.frame_count();
            let end = emu.timing_snapshot().now().get() + region.frame_ticks() * 2;
            while emu.timing_snapshot().now().get() < end {
                assert_eq!(
                    emu.step_instruction().unwrap(),
                    control.step_instruction().unwrap()
                );
            }
            assert!(emu.frame_count() > frame);
            assert_eq!(emu, control);
            assert_eq!(emu.video_error(), None);
            assert_eq!(emu.framebuffer(), control.framebuffer());
            let pcm = emu.drain_audio_samples();
            assert!(pcm.iter().any(|sample| *sample != 0.0));
            assert_eq!(pcm, control.drain_audio_samples());
            assert_eq!(emu.encode_state().unwrap(), control.encode_state().unwrap());
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_disassembly_rom_view_keeps_mixed_instruction_boundaries_and_targets() {
    let mut rom = vec![0; 0x10200];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x10000_u32.to_be_bytes());
    let words: [u16; 17] = [
        0x203c, 0x1234, 0x5678, 0x41f9, 0x00ff, 0x0040, 0x4eb9, 0x0001, 0x0040, 0x51c8, 0x0002,
        0x66fe, 0x48e7, 0x8001, 0x4ef9, 0x00ff, 0,
    ];
    for (i, word) in words.into_iter().enumerate() {
        rom[0x10000 + i * 2..0x10002 + i * 2].copy_from_slice(&word.to_be_bytes());
    }
    let emu = Emulator::new(rom, LoadConfig::default()).unwrap();
    let state = emu.encode_state().unwrap();
    let mut req = request();
    req.show_disassembler = true;
    req.show_memory_viewer = false;
    let view = collect_md_snapshot(&emu, &req, None)
        .disassembly_view
        .unwrap();
    assert_eq!(view.lines.len(), 26);
    let expected = [
        (0x10000, 6, "MOVE.L #$12345678,D0", None),
        (0x10006, 6, "LEA $00FF0040.L,A0", None),
        (0x1000c, 6, "JSR $00010040.L", Some(0x10040)),
        (0x10012, 4, "DBF D0,$00010016", Some(0x10016)),
        (0x10016, 2, "BNE.S $00010016", Some(0x10016)),
        (0x10018, 4, "MOVEM.L D0/A7,-(A7)", None),
        (0x1001c, 6, "JMP $00FF0000.L", Some(0xff0000)),
    ];
    for (line, (address, length, text, target)) in view.lines.iter().zip(expected) {
        assert_eq!(
            (
                line.address,
                line.bytes.len(),
                line.mnemonic.as_str(),
                line.control_target
            ),
            (address, length, text, target)
        );
        for (offset, byte) in line.bytes.iter().enumerate() {
            assert_eq!(Some(*byte), emu.peek(address + offset as u32));
        }
    }
    assert_eq!(emu.encode_state().unwrap(), state);
}
