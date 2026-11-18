use super::*;
use zeff_md_core::{Emulator, LoadConfig};

fn fixture(size: usize, blocks: &[(usize, &[u16])]) -> Emulator {
    let mut rom = vec![0; size];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    rom[0x100..0x110].copy_from_slice(b"SEGA SSF        ");
    rom[0x1f0] = b'U';
    rom[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    for &(address, words) in blocks {
        for (index, word) in words.iter().enumerate() {
            rom[address + index * 2..address + index * 2 + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    Emulator::new(rom, LoadConfig::default()).unwrap()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_source_disassembly_reaches_unmapped_extended_ssf_and_high_original_offsets_without_mapping_links()
 {
    let mut emu = fixture(
        0x600000,
        &[
            (0x200, &[0x13fc, 9, 0xa1, 0x30f3, 0x60fe, 0x4e71]),
            (0x400040, &[0x7003, 0x60fe]),
            (0x480040, &[0x7003, 0x60fe]),
        ],
    );
    assert_eq!(emu.peek(0x080040), Some(0));
    let before = emu.encode_debugger_checkpoint().unwrap();
    let unmapped = snapshot(emu.rom_bytes(), 0x480040);
    assert_eq!(unmapped.lines[0].mnemonic.as_str(), "MOVEQ #3,D0");
    assert_eq!(unmapped.lines[0].storage_offset, Some(0x480040));
    assert_eq!(emu.encode_debugger_checkpoint().unwrap(), before);
    emu.step_instruction().unwrap();
    assert_eq!(
        (emu.peek(0x080040), emu.peek(0x080041)),
        (Some(0x70), Some(3))
    );
    for offset in [0x400040, 0x480040] {
        let view = super::super::snapshot(
            &emu,
            true,
            Some(crate::debug::DisassemblyTarget::RomStorage(offset)),
        )
        .unwrap();
        assert_eq!(view.lines[0].storage_offset, Some(offset));
        assert_eq!(view.lines[0].bytes.as_slice(), [0x70, 3]);
        assert!(view.lines[1].control_target.is_none());
    }
    let high = fixture(0x2000000, &[(0x1000041, &[0x7004, 0x60fe])]);
    let before = high.encode_debugger_checkpoint().unwrap();
    let view = super::super::snapshot(
        &high,
        true,
        Some(crate::debug::DisassemblyTarget::RomStorage(0x1000041)),
    )
    .unwrap();
    assert_eq!(
        view.cursor,
        crate::debug::DisassemblyCursor::RomStorage(0x1000041)
    );
    assert_eq!(view.lines[0].storage_offset, Some(0x1000041));
    assert_eq!(view.lines[0].bytes.as_slice(), [0x70, 4]);
    assert_eq!(view.lines[1].mnemonic.as_str(), "BRA.S (PC+2-2)");
    assert!(high.supports_rom_cheats());
    let branch = &view.lines[1];
    assert!(
        branch.control_target.is_none()
            && branch.control_target_storage.is_none()
            && branch.control_target_bank.is_none()
    );
    assert_eq!(high.encode_debugger_checkpoint().unwrap(), before);
}
