use super::{Case, PREFIXES, literal_case};

pub(super) fn base_rows() {
    for prefixes in PREFIXES {
        for opcode in [0xa0, 0xa8] {
            for (af, value, flags) in [
                ([0, 1], 2, 0x25),
                ([0, 1], 0x20, 5),
                ([0xff, 0xff], 1, 0xc5),
                ([0, 0xff], 8, 0xcd),
            ] {
                literal_case(
                    prefixes,
                    Case {
                        opcode,
                        af,
                        pairs: [2, 0x0200, 0x0100],
                        value,
                        flags,
                        repeat: None,
                    },
                    0x4000,
                );
            }
        }
        for opcode in [0xa1, 0xa9] {
            for (af, value, flags) in [
                ([0x10, 1], 8, 0x37),
                ([0x80, 1], 0x80, 0x47),
                ([0, 0], 1, 0xbe),
                ([0x80, 0], 1, 0x3e),
            ] {
                literal_case(
                    prefixes,
                    Case {
                        opcode,
                        af,
                        pairs: [2, 0x0200, 0x0100],
                        value,
                        flags,
                        repeat: None,
                    },
                    0x4000,
                );
            }
        }
        for (opcodes, bc, hl, value, flags) in [
            ([0xa2, 0xb2], 0x01ff, 0x0100, 0xff, 0x42),
            ([0xaa, 0xba], 0x0100, 0x0100, 1, 0x55),
            ([0xa3, 0xb3], 0x0112, 0x00ff, 0xff, 0x42),
            ([0xab, 0xbb], 0x0112, 0x0000, 1, 0x55),
        ] {
            for opcode in opcodes {
                literal_case(
                    prefixes,
                    Case {
                        opcode,
                        af: [0x12, 0xff],
                        pairs: [bc, 0x0200, hl],
                        value,
                        flags,
                        repeat: None,
                    },
                    0x4000,
                );
            }
        }
    }
}

pub(super) fn repeat_rows() {
    for (opcodes, af, bc, hl, value, base, repeat) in [
        ([0xb0, 0xb8], [0, 1], 2, 0x0100, 2, 0x25, 0x2d),
        ([0xb1, 0xb9], [0x10, 1], 2, 0x0100, 8, 0x37, 0x3f),
    ] {
        for opcode in opcodes {
            literal_case(
                &[],
                Case {
                    opcode,
                    af,
                    pairs: [bc, 0x0200, hl],
                    value,
                    flags: base,
                    repeat: Some(repeat),
                },
                0x2800,
            );
        }
    }
    for (b, value, base, repeat, ports, addresses) in [
        (2, 1, 4, 0x28, [0, 2], [0x0100, 0x0102]),
        (2, 0xff, 0x17, 0x2f, [1, 3], [0x0101, 0x0103]),
        (0x11, 0xff, 0x17, 0x3b, [1, 3], [0x0101, 0x0103]),
        (2, 0x7f, 0x11, 0x2d, [0x80, 0x82], [0x0180, 0x0182]),
        (0x10, 0x7f, 0x1d, 0x3d, [0x80, 0x82], [0x0180, 0x0182]),
    ] {
        for (opcode, port, hl) in [
            (0xb2, ports[0], 0x0100),
            (0xba, ports[1], 0x0100),
            (0xb3, 0x12, addresses[0]),
            (0xbb, 0x12, addresses[1]),
        ] {
            literal_case(
                &[],
                Case {
                    opcode,
                    af: [0x12, 0xff],
                    pairs: [(b << 8) | port, 0x0200, hl],
                    value,
                    flags: base,
                    repeat: Some(repeat),
                },
                0x2800,
            );
        }
    }
    for (prefixes, start, repeat) in [
        (&[][..], 0x27ff, 0x25),
        (&[0xdd][..], 0x27ff, 0x2d),
        (&[0xfd][..], 0x27ff, 0x2d),
    ] {
        literal_case(
            prefixes,
            Case {
                opcode: 0xb0,
                af: [0, 1],
                pairs: [2, 0x0200, 0x0100],
                value: 2,
                flags: 0x25,
                repeat: Some(repeat),
            },
            start,
        );
    }
    literal_case(
        &[],
        Case {
            opcode: 0xb0,
            af: [0, 1],
            pairs: [2, 0x0200, 0x0100],
            value: 2,
            flags: 0x25,
            repeat: Some(0x2d),
        },
        0xffff,
    );
}
