use crate::axis::*;
use crate::literal::Value;

pub fn rows(section: u8, base: &Value, emit: &mut impl FnMut(Value)) {
    let (_, mode, reg) = geometry(base);
    if base.get("lane").number() != 0 {
        return;
    }
    match section {
        8 | 10 | 11 => {
            let desired = if section == 8 {
                6
            } else if section == 10 {
                5
            } else {
                7
            };
            if mode != desired || section == 11 && reg != 0 {
                return;
            }
            for extension in 0..65536u32 {
                emit(changed(
                    base,
                    vec![
                        ("preserve_base", b(true)),
                        ("extensions", list([n(extension)])),
                        ("extension_code", n(extension)),
                    ],
                ));
            }
        }
        9 if mode == 6 => {
            for selector in 0..16u32 {
                for width in 0..2u32 {
                    for ignored in 0..8u32 {
                        for offset in [0, 1, 0x80, 0xffu32] {
                            for path in 0..5u32 {
                                let extension =
                                    selector << 12 | width << 11 | ignored << 8 | offset;
                                let phase = [None, Some("E"), Some("RH"), Some("F"), Some("W")]
                                    [path as usize];
                                let mut waits = positional(base, 0);
                                if let Some(phase) = phase {
                                    set(&mut waits, phase, n(3u32));
                                }
                                emit(changed(
                                    base,
                                    vec![
                                        ("preserve_base", b(true)),
                                        ("extensions", list([n(extension)])),
                                        ("fail_phase", optional(phase)),
                                        ("waits", waits),
                                        ("path", n(path)),
                                    ],
                                ));
                            }
                        }
                    }
                }
            }
        }
        12 if (mode, reg) == (7, 1) => {
            for high in [0, 1, 0xff, 0x100, 0x7fff, 0x8000, 0xff00, 0xffffu32] {
                for low in [0, 1, 0xff, 0x100, 0x7fff, 0x8000, 0xff00, 0xffffu32] {
                    for phase in [
                        None,
                        Some("E1"),
                        Some("E2"),
                        Some("RH"),
                        Some("F"),
                        Some("W"),
                    ] {
                        emit(changed(
                            base,
                            vec![
                                ("extensions", list([n(high), n(low)])),
                                ("fail_phase", optional(phase)),
                                (
                                    "waits",
                                    phase.map_or_else(
                                        || object(vec![]),
                                        |p| object(vec![(p, n(3u32))]),
                                    ),
                                ),
                            ],
                        ));
                    }
                }
            }
        }
        _ => {}
    }
}
