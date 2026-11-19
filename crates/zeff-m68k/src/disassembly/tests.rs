use super::{DecodeStatus, DecodedInstruction, MAX_INSTRUCTION_BYTES, decode};
use std::cell::Cell;

fn at(address: u32, words: &[u16]) -> DecodedInstruction {
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_be_bytes()).collect();
    decode(address, |cursor| {
        bytes.get(cursor.wrapping_sub(address) as usize).copied()
    })
}

mod families;

#[test]
fn encoding_boundaries_have_exact_text_lengths_and_targets() {
    let vectors: &[(&[u16], &str, Option<u32>)] = &[
        (&[0x013c, 0x0080], "BTST.B D0,#$80", None),
        (
            &[0x083a, 0x0007, 0xfffc],
            "BTST.B #$0007,($00000100,PC)",
            None,
        ),
        (&[0x4cba, 0x0001, 0xfffc], "MOVEM.W ($00000100,PC),D0", None),
        (&[0x48e7, 0x0001], "MOVEM.L A7,-(A7)", None),
        (&[0x48e7, 0x8000], "MOVEM.L D0,-(A7)", None),
        (&[0x0699, 0x1234, 0x5678], "ADDI.L #$12345678,(A1)+", None),
        (
            &[0x06b9, 0x1234, 0x5678, 0x8765, 0x4320],
            "ADDI.L #$12345678,$87654320.L",
            None,
        ),
        (&[0x91c8], "SUBA.L A0,A0", None),
        (&[0xb1c8], "CMPA.L A0,A0", None),
        (&[0xc148], "EXG A0,A0", None),
        (&[0x60ff], "BRA.S $00000101", Some(0x101)),
        (&[0x51c8, 0x0000], "DBF D0,$00000102", Some(0x102)),
        (&[0x4ef8, 0x8001], "JMP $FFFF8001.W", Some(0xffff8001)),
    ];
    for (words, text, target) in vectors {
        let decoded = at(0x100, words);
        assert_eq!(decoded.status, DecodeStatus::Complete, "{words:04X?}");
        assert_eq!(&decoded.text, text, "{words:04X?}");
        assert_eq!(decoded.bytes.len(), words.len() * 2, "{words:04X?}");
        assert_eq!(decoded.control_target, *target, "{words:04X?}");
    }
}

#[test]
fn every_movem_mask_bit_preserves_direction_order() {
    let registers = [
        "D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7", "A0", "A1", "A2", "A3", "A4", "A5", "A6",
        "A7",
    ];
    for (index, register) in registers.into_iter().enumerate() {
        for (store, predecrement, load, size) in
            [(0x4890, 0x48a0, 0x4c98, "W"), (0x48d0, 0x48e0, 0x4cd8, "L")]
        {
            let normal_mask = 1u16 << index;
            let reversed_mask = 0x8000u16 >> index;
            for (words, expected) in [
                (
                    [store, normal_mask],
                    format!("MOVEM.{size} {register},(A0)"),
                ),
                (
                    [predecrement, reversed_mask],
                    format!("MOVEM.{size} {register},-(A0)"),
                ),
                (
                    [load, normal_mask],
                    format!("MOVEM.{size} (A0)+,{register}"),
                ),
            ] {
                let decoded = at(0x100, &words);
                assert_eq!(decoded.status, DecodeStatus::Complete);
                assert_eq!(decoded.text, expected);
                assert_eq!(decoded.bytes.len(), 4);
                assert_eq!(decoded.control_target, None);
            }
        }
    }
}

#[test]
fn static_targets_and_original_branch_displacements() {
    let vectors: &[(&[u16], Option<u32>)] = &[
        (&[0x4ef8, 0x8000], Some(0xffff8000)),
        (&[0x4eb9, 0x1234, 0x5678], Some(0x12345678)),
        (&[0x4efa, 0xfffc], Some(0xfe)),
        (&[0x4efb, 0x1000], None),
        (&[0x4ee8, 0x0004], None),
        (&[0x4ed0], None),
        (&[0x41f9, 0x1234, 0x5678], None),
        (&[0x4878, 0x1234], None),
        (&[0x6000, 0xfffe], Some(0x100)),
        (&[0x60ff], Some(0x101)),
        (&[0x6080], Some(0x82)),
        (&[0x607f], Some(0x181)),
        (&[0x51c8, 0xfffe], Some(0x100)),
    ];
    for (words, target) in vectors {
        assert_eq!(at(0x100, words).control_target, *target, "{words:04X?}");
    }
    assert_eq!(at(0xffff_fffe, &[0x6002]).control_target, Some(2));
    assert_eq!(
        at(0xffff_fffe, &[0x4efa, 0xfffe]).control_target,
        Some(0xffff_fffe)
    );
    let next = Cell::new(0);
    let wrapped = decode(0xffff_fffe, |address| {
        let index = next.get();
        assert_eq!(address, [0xffff_fffe, 0xffff_ffff, 0, 1][index]);
        next.set(index + 1);
        Some([0x4e, 0xfa, 0xff, 0xfe][index])
    });
    assert_eq!(next.get(), 4);
    assert_eq!(wrapped.text, "JMP ($FFFFFFFE,PC)");
    assert_eq!(wrapped.control_target, Some(0xffff_fffe));
    let physical = [0x60, 0x00, 0x00, 0x02];
    let decoded = decode(0x00ff_fffe, |address| match address & 0xffffff {
        0xfffffe => Some(physical[0]),
        0xffffff => Some(physical[1]),
        0 => Some(physical[2]),
        1 => Some(physical[3]),
        _ => None,
    });
    assert_eq!(decoded.bytes, physical);
    assert_eq!(
        decoded.control_target.map(|target| target & 0xffffff),
        Some(2)
    );
}

#[test]
fn invalid_encodings_do_not_read_extensions() {
    let invalid = [
        0x1008, 0x1040, 0x19c0, 0x003a, 0x00c0, 0x0e00, 0x083c, 0x42c0, 0x4a3a, 0x4a3c, 0x4e74,
        0x4e7a, 0x4e7b, 0x4e78, 0x49c0, 0x4848, 0x4888, 0x48d8, 0x48fa, 0x4ca0, 0x4cfc, 0x4e98,
        0x4ec0, 0x4afa, 0x4afb, 0x50fa, 0x5008, 0x7100, 0x8008, 0x9008, 0xb13a, 0xc008, 0xe8d0,
        0xe0c0, 0xa000, 0xffff, 0x0c3a, 0x40fa, 0x4110, 0x4c10, 0x8140, 0x50fc, 0x4c50, 0x0ad0,
        0x0cd0, 0x0ed0, 0x0cfc, 0x0efc, 0x8180, 0x06d0, 0x06c0,
    ];
    for op in invalid {
        let reads = Cell::new(0);
        let decoded = decode(0x100, |address| {
            reads.set(reads.get() + 1);
            Some(match address {
                0x100 => (op >> 8) as u8,
                0x101 => op as u8,
                _ => 0,
            })
        });
        assert_eq!(
            decoded.status,
            DecodeStatus::Invalid,
            "{op:04X}: {}",
            decoded.text
        );
        assert_eq!(decoded.bytes.len(), 2, "{op:04X}");
        assert_eq!(reads.get(), 2, "{op:04X}");
        assert_eq!(decoded.text, format!("DC.W ${op:04X}"));
    }
}

#[test]
fn truncation_preserves_only_actual_bytes_and_stops_at_the_hole() {
    let vectors: &[&[u16]] = &[
        &[0x23fc, 0x1234, 0x5678, 0x8765, 0x4320],
        &[0x23f9, 0x1234, 0x5678, 0x8765, 0x4320],
        &[0x48f9, 0xffff, 0x1234, 0x5678],
        &[0x4cba, 0x0123, 0xfffe],
        &[0x0a90, 0x1234, 0x5678],
        &[0x4e72, 0x2700],
        &[0x6000, 0xfffe],
        &[0x51c8, 0xfffe],
        &[0x1030, 0x20fe],
        &[0x0839, 0x0007, 0x1234, 0x5678],
        &[0x013c, 0x0080],
        &[0x083a, 0x0007, 0xfffc],
        &[0x06b9, 0x1234, 0x5678, 0x8765, 0x4320],
    ];
    for words in vectors {
        let bytes: Vec<_> = words.iter().flat_map(|word| word.to_be_bytes()).collect();
        for hole in 0..bytes.len() {
            let reads = Cell::new(0);
            let decoded = decode(0x100, |address| {
                let index = address.wrapping_sub(0x100) as usize;
                assert!(index <= hole, "read after hole");
                reads.set(reads.get() + 1);
                if index == hole {
                    None
                } else {
                    bytes.get(index).copied()
                }
            });
            assert_eq!(
                decoded.status,
                DecodeStatus::Unavailable,
                "{words:04X?}, hole {hole}"
            );
            assert_eq!(decoded.bytes, bytes[..hole]);
            assert_eq!(decoded.control_target, None);
            assert_eq!(reads.get(), hole + 1);
        }
    }
}

#[test]
fn odd_addresses_do_not_read_and_text_has_no_fixed_capacity_truncation() {
    let decoded = decode(1, |_| panic!("unaligned read"));
    assert_eq!(decoded.status, DecodeStatus::Unaligned);
    assert!(decoded.bytes.is_empty());
    let decoded = at(0, &[0x48f0, 0x5555, 0xf8ff]);
    assert_eq!(decoded.text, "MOVEM.L D0/D2/D4/D6/A0/A2/A4/A6,(-1,A0,A7.L)");
    assert!(decoded.text.len() > 32);
    assert_eq!(at(0, &[0x48e7, 0]).text, "MOVEM.L #0,-(A7)");
}

#[test]
fn all_opcode_words_have_bounded_reads_and_progress() {
    for extension in [0x00, 0x55, 0xff] {
        for op in 0..=u16::MAX {
            let reads = Cell::new(0);
            let decoded = decode(0xffff_fffc, |address| {
                let index = address.wrapping_sub(0xffff_fffc) as usize;
                assert!(index < MAX_INSTRUCTION_BYTES, "{op:04X}");
                assert_eq!(index, reads.get());
                reads.set(reads.get() + 1);
                Some(match index {
                    0 => (op >> 8) as u8,
                    1 => op as u8,
                    _ => extension,
                })
            });
            assert!(matches!(
                decoded.status,
                DecodeStatus::Complete | DecodeStatus::Invalid
            ));
            assert!((2..=MAX_INSTRUCTION_BYTES).contains(&decoded.bytes.len()));
            assert_eq!(decoded.bytes.len() % 2, 0);
            assert_eq!(reads.get(), decoded.bytes.len(), "{op:04X}");
            if decoded.status == DecodeStatus::Invalid {
                assert_eq!(decoded.bytes.len(), 2);
            }
        }
    }
}
