use crate::axis::*;
use crate::literal::Value;

pub fn rows(section: u8, base: &Value, emit: &mut impl FnMut(Value)) {
    let (size, mode, _) = geometry(base);
    let bits = [8, 16, 32][size];
    let sign = 1u32 << (bits - 1);
    match section {
        0 => {
            for ccr in 0..32u32 {
                for mask in 0..8u32 {
                    for value in VALUES[size] {
                        for wait in 0..2u32 {
                            emit(changed(
                                base,
                                vec![
                                    ("CCR", n(ccr)),
                                    ("mask", n(mask)),
                                    ("value", n(value)),
                                    (
                                        "waits",
                                        if wait == 0 {
                                            object(vec![])
                                        } else {
                                            positional(base, 0)
                                        },
                                    ),
                                    ("wait_variant", n(wait)),
                                ],
                            ));
                        }
                    }
                }
            }
        }
        1 => {
            for phase in names(base) {
                for ccr in 0..32u32 {
                    for value in VALUES[size] {
                        for wait in [0, 1, 3, u32::MAX] {
                            emit(changed(
                                base,
                                vec![
                                    ("fail_phase", s(phase)),
                                    ("CCR", n(ccr)),
                                    ("value", n(value)),
                                    ("waits", object(vec![(phase, n(wait))])),
                                ],
                            ));
                        }
                    }
                }
            }
        }
        2 => {
            for phase in names(base) {
                for mask in 0..8u32 {
                    for value in [0, sign] {
                        for wait in [0, 1, 3, u32::MAX] {
                            emit(changed(
                                base,
                                vec![
                                    ("fail_phase", s(phase)),
                                    ("CCR", n(31u32)),
                                    ("mask", n(mask)),
                                    ("value", n(value)),
                                    ("waits", object(vec![(phase, n(wait))])),
                                ],
                            ));
                        }
                    }
                }
            }
        }
        3 => {
            for phase in names(base) {
                for ccr in [0, 15, 16, 31u32] {
                    for value in [0, sign] {
                        for failed in [false, true] {
                            for vector in 0..2u32 {
                                emit(changed(
                                    base,
                                    vec![
                                        ("selected_phase", s(phase)),
                                        ("CCR", n(ccr)),
                                        ("value", n(value)),
                                        ("fail_phase", optional(failed.then_some(phase))),
                                        ("waits", positional(base, vector as usize)),
                                        ("wait_variant", n(vector)),
                                    ],
                                ));
                            }
                        }
                    }
                }
            }
        }
        4 if size == 0 => {
            for value in 0..256u32 {
                emit(changed(
                    base,
                    vec![
                        ("value", n(value)),
                        ("CCR", n((value & 1) << 4)),
                        ("upper", n(0xa5b6c700u32)),
                        ("other_byte", n(0x5au32)),
                    ],
                ));
            }
        }
        5 if size == 2 => {
            for ccr in 0..32u32 {
                for high in [0, 1, 0x8000, 0xffffu32] {
                    for low in [0, 1, 0x8000, 0xffffu32] {
                        emit(changed(
                            base,
                            vec![("CCR", n(ccr)), ("value", n(high << 16 | low))],
                        ));
                    }
                }
            }
        }
        6 if mode == 0 && size < 2 => {
            for ccr in 0..32u32 {
                for value in [0, 1, sign, (1u32 << bits) - 1] {
                    for upper in if size == 0 {
                        [0, 1, 0x800000, 0xffffffu32]
                    } else {
                        [0, 1, 0x8000, 0xffffu32]
                    } {
                        emit(changed(
                            base,
                            vec![
                                ("CCR", n(ccr)),
                                ("value", n(value)),
                                ("upper", n(upper << bits)),
                            ],
                        ));
                    }
                }
            }
        }
        24 if mode != 0 => {
            for ccr in 0..32u32 {
                for value in VALUES[size] {
                    for wait in [0, 1, 3, u32::MAX] {
                        emit(changed(
                            base,
                            vec![
                                ("CCR", n(ccr)),
                                ("value", n(value)),
                                ("fail_phase", s("W")),
                                ("waits", object(vec![("W", n(wait))])),
                                ("failed_write_deposit", b(true)),
                            ],
                        ));
                    }
                }
            }
        }
        25 => {
            for pc in [0x2001, u32::MAX, 0x00ffffff] {
                emit(changed(base, vec![("PC", n(pc))]));
            }
        }
        _ => {}
    }
}
