use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_original_rom_disassembly_uses_exact_odd_bytes_and_symbolic_pc_without_links() {
    let bytes = [
        0xff, 0x70, 0x01, 0x60, 0xfe, 0x4e, 0xb9, 0xab, 0x00, 0x02, 0x00, 0x20,
    ];
    let view = snapshot(&bytes, 1);
    assert_eq!(view.cursor, crate::debug::DisassemblyCursor::RomStorage(1));
    assert!(view.is_static_target && view.is_navigation_target);
    assert_eq!(view.lines.len(), 4);
    let expected = [
        (1, &[0x70, 1][..], "MOVEQ #1,D0", DecodeStatus::Complete),
        (
            3,
            &[0x60, 0xfe][..],
            "BRA.S (PC+2-2)",
            DecodeStatus::Complete,
        ),
        (
            5,
            &[0x4e, 0xb9, 0xab, 0, 2, 0][..],
            "JSR $AB000200.L",
            DecodeStatus::Complete,
        ),
        (11, &[0x20][..], "<unavailable>", DecodeStatus::Unavailable),
    ];
    for (line, (offset, bytes, text, status)) in view.lines.iter().zip(expected) {
        assert_eq!(line.storage_offset, Some(offset));
        assert_eq!(line.bytes.as_slice(), bytes);
        assert_eq!(line.mnemonic.as_str(), text);
        assert_eq!(line.decode_status, Some(status));
        assert!(
            line.bank.is_none()
                && line.control_target.is_none()
                && line.control_target_storage.is_none()
                && line.control_target_bank.is_none()
        );
    }
    for offset in [12, 13, u64::from(u32::MAX) + 1, u64::MAX] {
        let empty = snapshot(&bytes, offset);
        assert_eq!(
            empty.cursor,
            crate::debug::DisassemblyCursor::RomStorage(offset)
        );
        assert!(empty.lines.is_empty());
        assert!(empty.cursor.cpu_address().is_none());
    }
    let page = snapshot(&[0x4e, 0x71].repeat(40), 0);
    assert_eq!(page.lines.len(), 26);
    assert_eq!(page.lines[25].storage_offset, Some(50));
    let invalid = snapshot(&[0x71, 0, 0x4e, 0x71], 0);
    assert_eq!(invalid.lines[0].decode_status, Some(DecodeStatus::Invalid));
    assert_eq!(invalid.lines[0].mnemonic.as_str(), "DC.W $7100");
    assert_eq!(invalid.lines[1].storage_offset, Some(2));
    assert_eq!(invalid.lines[1].mnemonic.as_str(), "NOP");
}
