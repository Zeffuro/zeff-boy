use super::{DecodeStatus, at};

#[test]
fn original_family_coverage_ledger() {
    let vectors: &[(&[u16], &str)] = &[
        (&[0x103c, 0x1234], "MOVE.B #$34,D0"),
        (&[0x3208], "MOVE.W A0,D1"),
        (&[0x247c, 0x1234, 0x5678], "MOVEA.L #$12345678,A2"),
        (&[0x70ff], "MOVEQ #-1,D0"),
        (&[0x0000, 0xabcd], "ORI.B #$CD,D0"),
        (&[0x0240, 0x1234], "ANDI.W #$1234,D0"),
        (&[0x0480, 0x1234, 0x5678], "SUBI.L #$12345678,D0"),
        (&[0x0600, 0x0034], "ADDI.B #$34,D0"),
        (&[0x0a40, 0x1234], "EORI.W #$1234,D0"),
        (&[0x0c80, 0x1234, 0x5678], "CMPI.L #$12345678,D0"),
        (&[0x003c, 0xabcd], "ORI #$CD,CCR"),
        (&[0x027c, 0x1234], "ANDI #$1234,SR"),
        (&[0x0a3c, 0x1234], "EORI #$34,CCR"),
        (&[0x054b, 0xfffc], "MOVEP.L (-4,A3),D2"),
        (&[0x0188, 0x0004], "MOVEP.W D0,(4,A0)"),
        (&[0x0800, 0x0007], "BTST.L #$0007,D0"),
        (&[0x0150], "BCHG.B D0,(A0)"),
        (&[0x0890, 0x0003], "BCLR.B #$0003,(A0)"),
        (&[0x01c0], "BSET.L D0,D0"),
        (&[0x41f9, 0x1234, 0x5678], "LEA $12345678.L,A0"),
        (&[0x4190], "CHK.W (A0),D0"),
        (&[0x4850], "PEA (A0)"),
        (&[0x4843], "SWAP D3"),
        (&[0x4883], "EXT.W D3"),
        (&[0x48c3], "EXT.L D3"),
        (&[0x48e7, 0xe081], "MOVEM.L D0-D2/A0/A7,-(A7)"),
        (&[0x4c9b, 0x0205], "MOVEM.W (A3)+,D0/D2/A1"),
        (&[0x40c0], "MOVE.W SR,D0"),
        (&[0x44fc, 0x001f], "MOVE.W #$001F,CCR"),
        (&[0x46d0], "MOVE.W (A0),SR"),
        (&[0x4000], "NEGX.B D0"),
        (&[0x4240], "CLR.W D0"),
        (&[0x4480], "NEG.L D0"),
        (&[0x4610], "NOT.B (A0)"),
        (&[0x4a50], "TST.W (A0)"),
        (&[0x4800], "NBCD D0"),
        (&[0x4ad0], "TAS (A0)"),
        (&[0x4e40], "TRAP #0"),
        (&[0x4e57, 0xfff0], "LINK A7,#-16"),
        (&[0x4e5f], "UNLK A7"),
        (&[0x4e60], "MOVE A0,USP"),
        (&[0x4e68], "MOVE USP,A0"),
        (&[0x4e70], "RESET"),
        (&[0x4e71], "NOP"),
        (&[0x4e72, 0x2700], "STOP #$2700"),
        (&[0x4e73], "RTE"),
        (&[0x4e75], "RTS"),
        (&[0x4e76], "TRAPV"),
        (&[0x4e77], "RTR"),
        (&[0x4afc], "ILLEGAL"),
        (&[0x4e90], "JSR (A0)"),
        (&[0x4ed0], "JMP (A0)"),
        (&[0x5088], "ADDQ.L #8,A0"),
        (&[0x5350], "SUBQ.W #1,(A0)"),
        (&[0x56c0], "SNE D0"),
        (&[0x56ca, 0xfffc], "DBNE D2,$000000FE"),
        (&[0x6002], "BRA.S $00000104"),
        (&[0x6100, 0xfffc], "BSR.W $000000FE"),
        (&[0x66ff], "BNE.S $00000101"),
        (&[0x8040], "OR.W D0,D0"),
        (&[0x8150], "OR.W D0,(A0)"),
        (&[0x80d0], "DIVU.W (A0),D0"),
        (&[0x81d0], "DIVS.W (A0),D0"),
        (&[0x8101], "SBCD D1,D0"),
        (&[0x8109], "SBCD -(A1),-(A0)"),
        (&[0x9040], "SUB.W D0,D0"),
        (&[0x91a8, 0x0004], "SUB.L D0,(4,A0)"),
        (&[0x90c8], "SUBA.W A0,A0"),
        (&[0x9181], "SUBX.L D1,D0"),
        (&[0xb040], "CMP.W D0,D0"),
        (&[0xb1c8], "CMPA.L A0,A0"),
        (&[0xb589], "CMPM.L (A1)+,(A2)+"),
        (&[0xb100], "EOR.B D0,D0"),
        (&[0xc040], "AND.W D0,D0"),
        (&[0xc150], "AND.W D0,(A0)"),
        (&[0xc0d0], "MULU.W (A0),D0"),
        (&[0xc1d0], "MULS.W (A0),D0"),
        (&[0xc101], "ABCD D1,D0"),
        (&[0xc109], "ABCD -(A1),-(A0)"),
        (&[0xc141], "EXG D0,D1"),
        (&[0xc149], "EXG A0,A1"),
        (&[0xc189], "EXG D0,A1"),
        (&[0xd489], "ADD.L A1,D2"),
        (&[0xd150], "ADD.W D0,(A0)"),
        (&[0xd4c3], "ADDA.W D3,A2"),
        (&[0xd581], "ADDX.L D1,D2"),
        (&[0xd589], "ADDX.L -(A1),-(A2)"),
        (&[0xe000], "ASR.B #8,D0"),
        (&[0xe148], "LSL.W #8,D0"),
        (&[0xe2b0], "ROXR.L D1,D0"),
        (&[0xe398], "ROL.L #1,D0"),
        (&[0xe0d0], "ASR.W (A0)"),
        (&[0xe3d0], "LSL.W (A0)"),
        (&[0xe4d0], "ROXR.W (A0)"),
        (&[0xe7d0], "ROL.W (A0)"),
    ];
    for (words, expected) in vectors {
        let decoded = at(0x100, words);
        assert_eq!(decoded.status, DecodeStatus::Complete, "{words:04X?}");
        assert_eq!(&decoded.text, expected, "{words:04X?}");
        assert_eq!(decoded.bytes.len(), words.len() * 2, "{words:04X?}");
    }
}

#[test]
fn effective_address_extensions_and_sizes() {
    let vectors: &[(&[u16], &str)] = &[
        (&[0x1000], "MOVE.B D0,D0"),
        (&[0x3008], "MOVE.W A0,D0"),
        (&[0x2010], "MOVE.L (A0),D0"),
        (&[0x1018], "MOVE.B (A0)+,D0"),
        (&[0x3020], "MOVE.W -(A0),D0"),
        (&[0x2028, 0xfffc], "MOVE.L (-4,A0),D0"),
        (&[0x1030, 0x20fe], "MOVE.B (-2,A0,D2.W),D0"),
        (&[0x3030, 0xb8fe], "MOVE.W (-2,A0,A3.L),D0"),
        (&[0x2038, 0x8000], "MOVE.L $FFFF8000.W,D0"),
        (&[0x1039, 0x1234, 0x5678], "MOVE.B $12345678.L,D0"),
        (&[0x303a, 0xfffc], "MOVE.W ($000000FE,PC),D0"),
        (&[0x203b, 0x18fe], "MOVE.L (-2,PC,D1.L),D0"),
        (&[0x103c, 0xff80], "MOVE.B #$80,D0"),
        (&[0x303c, 0xff80], "MOVE.W #$FF80,D0"),
        (&[0x203c, 0x1234, 0x5678], "MOVE.L #$12345678,D0"),
        (
            &[0x23f9, 0x1234, 0x5678, 0x8765, 0x4320],
            "MOVE.L $12345678.L,$87654320.L",
        ),
        (
            &[0x23fc, 0x1234, 0x5678, 0x8765, 0x4320],
            "MOVE.L #$12345678,$87654320.L",
        ),
        (&[0x4cba, 0x0001, 0xfffc], "MOVEM.W ($00000100,PC),D0"),
    ];
    for (words, text) in vectors {
        let decoded = at(0x100, words);
        assert_eq!(decoded.status, DecodeStatus::Complete);
        assert_eq!(&decoded.text, text);
        assert_eq!(decoded.bytes.len(), words.len() * 2);
    }
    assert_eq!(
        at(0x100, &[0x2030, 0x18fe]).text,
        at(0x100, &[0x2030, 0x1ffe]).text
    );
    assert_eq!(
        at(0x100, &[0x203b, 0x18fe]).text,
        at(0x100, &[0x203b, 0x1ffe]).text
    );
}

#[test]
fn sized_families_cover_byte_word_and_long() {
    let families = [
        (0x0000, "ORI", true),
        (0x0200, "ANDI", true),
        (0x0400, "SUBI", true),
        (0x0600, "ADDI", true),
        (0x0a00, "EORI", true),
        (0x0c00, "CMPI", true),
        (0x4000, "NEGX", false),
        (0x4200, "CLR", false),
        (0x4400, "NEG", false),
        (0x4600, "NOT", false),
        (0x4a00, "TST", false),
    ];
    for (base, name, immediate) in families {
        for (field, suffix, value, count) in [
            (0, "B", "#$34", 2),
            (0x40, "W", "#$1234", 2),
            (0x80, "L", "#$12345678", 3),
        ] {
            let words = [base | field, 0x1234, 0x5678];
            let decoded = at(0, &words);
            assert_eq!(decoded.status, DecodeStatus::Complete);
            let expected = if immediate {
                format!("{name}.{suffix} {value},D0")
            } else {
                format!("{name}.{suffix} D0")
            };
            assert_eq!(decoded.text, expected);
            assert_eq!(decoded.bytes.len(), if immediate { count * 2 } else { 2 });
        }
    }
    let families = [
        (0x8000, "OR", "D0,D0"),
        (0x9000, "SUB", "D0,D0"),
        (0xb000, "CMP", "D0,D0"),
        (0xb100, "EOR", "D0,D0"),
        (0xc000, "AND", "D0,D0"),
        (0xd000, "ADD", "D0,D0"),
        (0x9100, "SUBX", "D0,D0"),
        (0xd100, "ADDX", "D0,D0"),
        (0xb108, "CMPM", "(A0)+,(A0)+"),
        (0x5200, "ADDQ", "#1,D0"),
        (0x5300, "SUBQ", "#1,D0"),
        (0xe000, "ASR", "#8,D0"),
        (0xe100, "ASL", "#8,D0"),
        (0xe008, "LSR", "#8,D0"),
        (0xe108, "LSL", "#8,D0"),
        (0xe010, "ROXR", "#8,D0"),
        (0xe110, "ROXL", "#8,D0"),
        (0xe018, "ROR", "#8,D0"),
        (0xe118, "ROL", "#8,D0"),
    ];
    for (base, name, operands) in families {
        for (field, suffix) in [(0, "B"), (0x40, "W"), (0x80, "L")] {
            let decoded = at(0, &[base | field]);
            assert_eq!(
                decoded.status,
                DecodeStatus::Complete,
                "{:04X}",
                base | field
            );
            assert_eq!(decoded.text, format!("{name}.{suffix} {operands}"));
        }
    }
    for (base, name) in [(0x90c0, "SUBA"), (0xb0c0, "CMPA"), (0xd0c0, "ADDA")] {
        assert_eq!(at(0, &[base]).text, format!("{name}.W D0,A0"));
        assert_eq!(at(0, &[base | 0x100]).text, format!("{name}.L D0,A0"));
    }
}

#[test]
fn movem_directions_and_condition_names() {
    let store_modes: &[(&[u16], &str)] = &[
        (&[0x4890, 0x0001], "(A0)"),
        (&[0x48a0, 0x8000], "-(A0)"),
        (&[0x48a8, 0x0001, 0x0004], "(4,A0)"),
        (&[0x48b0, 0x0001, 0x1004], "(4,A0,D1.W)"),
        (&[0x48b8, 0x0001, 0x1234], "$00001234.W"),
        (&[0x48b9, 0x0001, 0x1234, 0x5678], "$12345678.L"),
    ];
    for (words, ea) in store_modes {
        assert_eq!(at(0, words).text, format!("MOVEM.W D0,{ea}"));
        let mut long = words.to_vec();
        long[0] |= 0x40;
        assert_eq!(at(0, &long).text, format!("MOVEM.L D0,{ea}"));
    }
    let load_modes: &[(&[u16], &str)] = &[
        (&[0x4c90, 0x0001], "(A0)"),
        (&[0x4c98, 0x0001], "(A0)+"),
        (&[0x4ca8, 0x0001, 0x0004], "(4,A0)"),
        (&[0x4cb0, 0x0001, 0x1004], "(4,A0,D1.W)"),
        (&[0x4cb8, 0x0001, 0x1234], "$00001234.W"),
        (&[0x4cb9, 0x0001, 0x1234, 0x5678], "$12345678.L"),
        (&[0x4cba, 0x0001, 0x0004], "($00000008,PC)"),
        (&[0x4cbb, 0x0001, 0x1004], "(4,PC,D1.W)"),
    ];
    for (words, ea) in load_modes {
        assert_eq!(at(0, words).text, format!("MOVEM.W {ea},D0"));
        let mut long = words.to_vec();
        long[0] |= 0x40;
        assert_eq!(at(0, &long).text, format!("MOVEM.L {ea},D0"));
    }
    for (code, suffix) in [
        "T", "F", "HI", "LS", "CC", "CS", "NE", "EQ", "VC", "VS", "PL", "MI", "GE", "LT", "GT",
        "LE",
    ]
    .into_iter()
    .enumerate()
    {
        let field = (code as u16) << 8;
        assert_eq!(at(0, &[0x50c0 | field]).text, format!("S{suffix} D0"));
        assert_eq!(
            at(0, &[0x50c8 | field, 0x0002]).text,
            format!("DB{suffix} D0,$00000004")
        );
        let name = match code {
            0 => "BRA".to_owned(),
            1 => "BSR".to_owned(),
            _ => format!("B{suffix}"),
        };
        assert_eq!(at(0, &[0x6002 | field]).text, format!("{name}.S $00000004"));
    }
}
