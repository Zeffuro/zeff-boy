use super::*;
use std::cell::{Cell, RefCell};

fn bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_be_bytes()).collect()
}

fn complete(words: &[u16], expected: &str) {
    let source = bytes(words);
    let UnmappedInstruction {
        bytes,
        text,
        status,
    } = decode_unmapped(&source);
    assert_eq!(status, DecodeStatus::Complete, "{words:04X?}");
    assert_eq!(bytes, source, "{words:04X?}");
    assert_eq!(text, expected, "{words:04X?}");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unmapped_branches_keep_symbolic_pc_and_original_signed_displacements() {
    for (words, text) in [
        (&[0x60fe][..], "BRA.S (PC+2-2)"),
        (&[0x607f][..], "BRA.S (PC+2+127)"),
        (&[0x6080][..], "BRA.S (PC+2-128)"),
        (&[0x60ff][..], "BRA.S (PC+2-1)"),
        (&[0x6000, 0xfffe][..], "BRA.W (PC+2-2)"),
        (&[0x6000, 0x7fff][..], "BRA.W (PC+2+32767)"),
        (&[0x6102][..], "BSR.S (PC+2+2)"),
        (&[0x6100, 0x8000][..], "BSR.W (PC+2-32768)"),
        (&[0x66fc][..], "BNE.S (PC+2-4)"),
        (&[0x51c8, 0xfffe][..], "DBF D0,(PC+2-2)"),
        (&[0x5dcf, 0][..], "DBLT D7,(PC+2+0)"),
    ] {
        complete(words, text);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unmapped_pc_operands_keep_displacement_after_masks_and_immediates() {
    for (words, text) in [
        (&[0x303a, 0xfffc][..], "MOVE.W (-4,PC),D0"),
        (&[0x4cba, 1, 0xfffc][..], "MOVEM.W (-4,PC),D0"),
        (&[0x083a, 7, 0xfffc][..], "BTST.B #$0007,(-4,PC)"),
        (&[0x4cbb, 1, 0x10fe][..], "MOVEM.W (-2,PC,D1.W),D0"),
        (&[0x083b, 7, 0x10fe][..], "BTST.B #$0007,(-2,PC,D1.W)"),
        (&[0x4eba, 0xfffc][..], "JSR (-4,PC)"),
        (&[0x4efa, 6][..], "JMP (6,PC)"),
        (&[0x4efb, 0x10fe][..], "JMP (-2,PC,D1.W)"),
        (&[0x41fa, 0xfff8][..], "LEA (-8,PC),A0"),
        (&[0x487a, 0][..], "PEA (0,PC)"),
    ] {
        complete(words, text);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unmapped_absolute_cpu_operands_and_non_pc_instructions_keep_literal_text() {
    for (words, text) in [
        (&[0x4ef8, 0x8001][..], "JMP $FFFF8001.W"),
        (&[0x4eb9, 0xab00, 0x0200][..], "JSR $AB000200.L"),
        (&[0x4ef9, 0xffff, 0x0200][..], "JMP $FFFF0200.L"),
        (&[0x4ed0][..], "JMP (A0)"),
        (&[0x303c, 0x1122][..], "MOVE.W #$1122,D0"),
        (&[0x48e7, 0x8001][..], "MOVEM.L D0/A7,-(A7)"),
        (&[0x203c, 0x1234, 0x5678][..], "MOVE.L #$12345678,D0"),
        (
            &[0x23fc, 0x1234, 0x5678, 0x8765, 0x4320][..],
            "MOVE.L #$12345678,$87654320.L",
        ),
        (&[0x4e71][..], "NOP"),
    ] {
        complete(words, text);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unmapped_slices_preserve_odd_starts_tails_invalid_fallback_and_read_bound() {
    let source = [0xa5, 0x4e, 0x71, 0xff];
    assert_eq!(decode_unmapped(&source[1..]).text, "NOP");
    assert_eq!(decode_unmapped(&source[1..]).bytes, [0x4e, 0x71]);
    assert_eq!(source, [0xa5, 0x4e, 0x71, 0xff]);
    for words in [
        &[0x23fc, 0x1234, 0x5678, 0x8765, 0x4320][..],
        &[0x4cba, 1, 0xfffc][..],
        &[0x083a, 7, 0xfffc][..],
        &[0x4eb9, 0xab00, 0x0200][..],
        &[0x6000, 0xfffe][..],
        &[0x51c8, 0][..],
    ] {
        let source = bytes(words);
        for tail in 0..source.len() {
            let decoded = decode_unmapped(&source[..tail]);
            assert_eq!(
                decoded.status,
                DecodeStatus::Unavailable,
                "{words:04X?} {tail}"
            );
            assert_eq!(decoded.bytes, source[..tail]);
            assert_eq!(decoded.text, "<unavailable>");
        }
    }
    let invalid = decode_unmapped(&[0x71, 0, 0x4e, 0x71]);
    assert_eq!(invalid.status, DecodeStatus::Invalid);
    assert_eq!(invalid.bytes, [0x71, 0]);
    assert_eq!(invalid.text, "DC.W $7100");

    let source = [
        0x23, 0xfc, 0x12, 0x34, 0x56, 0x78, 0x87, 0x65, 0x43, 0x20, 0xff, 0xff,
    ];
    let reads = Cell::new(0);
    let mut parser = Decoder {
        reader: |index: usize| {
            assert_eq!(index, reads.get());
            assert!(index < 10, "read beyond one instruction");
            reads.set(index + 1);
            Some(source[index])
        },
        logical_pc: None,
        bytes: Vec::new(),
        target: None,
    };
    assert_eq!(
        parser.parse(),
        (
            DecodeStatus::Complete,
            "MOVE.L #$12345678,$87654320.L".into()
        )
    );
    assert_eq!(reads.get(), 10);
    assert_eq!(parser.bytes, source[..10]);
    assert_eq!(decode_unmapped(&source).bytes, source[..10]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapped_decoder_retains_full_pc_wrap_reader_order_and_alignment() {
    let seen = RefCell::new(Vec::new());
    let source = [0x4c, 0xba, 0, 1, 0xff, 0xfc];
    let decoded = decode(0xffff_fffc, |address| {
        let index = seen.borrow().len();
        seen.borrow_mut().push(address);
        source.get(index).copied()
    });
    assert_eq!(
        *seen.borrow(),
        [0xffff_fffc, 0xffff_fffd, 0xffff_fffe, 0xffff_ffff, 0, 1]
    );
    assert_eq!(decoded.address, 0xffff_fffc);
    assert_eq!(decoded.bytes, source);
    assert_eq!(decoded.text, "MOVEM.W ($FFFFFFFC,PC),D0");
    assert_eq!(decoded.control_target, None);
    assert_eq!(decoded.status, DecodeStatus::Complete);

    let source = [0x60, 2];
    let decoded = decode(0xabff_fffe, |address| {
        source
            .get(address.wrapping_sub(0xabff_fffe) as usize)
            .copied()
    });
    assert_eq!(decoded.address, 0xabff_fffe);
    assert_eq!(decoded.text, "BRA.S $AC000002");
    assert_eq!(decoded.control_target, Some(0xac00_0002));
    let source = [0x4e, 0xfa, 0xff, 0xfe];
    let decoded = decode(0xffff_fffe, |address| {
        source
            .get(address.wrapping_sub(0xffff_fffe) as usize)
            .copied()
    });
    assert_eq!(decoded.text, "JMP ($FFFFFFFE,PC)");
    assert_eq!(decoded.control_target, Some(0xffff_fffe));
    let decoded = decode(0x00ff_fffe, |address| match address & 0xffffff {
        0xfffffe => Some(0x60),
        0xffffff => Some(0),
        0 => Some(0),
        1 => Some(2),
        _ => None,
    });
    assert_eq!(decoded.text, "BRA.W $01000002");
    assert_eq!(decoded.control_target, Some(0x0100_0002));
    assert_eq!(decoded.bytes, [0x60, 0, 0, 2]);
    let decoded = decode(1, |_| panic!("unaligned CPU decode read a byte"));
    assert_eq!(decoded.address, 1);
    assert_eq!(decoded.status, DecodeStatus::Unaligned);
    assert!(decoded.bytes.is_empty());
    assert_eq!(decoded.text, "<unaligned>");
    assert_eq!(decoded.control_target, None);
}
