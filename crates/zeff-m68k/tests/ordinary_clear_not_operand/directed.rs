use crate::axis::*;
use crate::literal::Value;
use crate::seed::RESERVE;

pub fn rows(section: u8, base: &Value, emit: &mut impl FnMut(Value)) {
    let (size, mode, reg) = geometry(base);
    let phases = names(base);
    match section {
        7 if (2..=6).contains(&mode) && reg == 7 => {
            for inactive in [0, 1, 0x12345678, u32::MAX] {
                let key = if base.get("bank").number() == 1 {
                    "usp"
                } else {
                    "ssp"
                };
                paths(
                    &changed(
                        base,
                        vec![("register_overrides", object(vec![(key, n(inactive))]))],
                    ),
                    "F",
                    emit,
                );
            }
        }
        13 => {
            for pc in [
                0x2000,
                0xfffffffa,
                0xfffffffc,
                0xfffffffe,
                0x00fffffc,
                0x00fffffeu32,
            ] {
                paths(&changed(base, vec![("PC", n(pc))]), "F", emit);
            }
        }
        14 if mode != 0 => {
            for ea in [
                0,
                1,
                0x00fffffe,
                0x00ffffff,
                0xfffffffe,
                u32::MAX,
                0x80000000,
                0x80000001,
            ] {
                paths(&changed(base, vec![("requested_EA", n(ea))]), "RH", emit);
            }
        }
        15 => {
            let seed = cfg(base);
            let anchor = seed.r.pc.wrapping_add(2 + 2 * seed.extensions as u32);
            for phase in phases {
                let logical = match phase {
                    "E" | "E1" => seed.r.pc.wrapping_add(4),
                    "E2" => seed.r.pc.wrapping_add(6),
                    "F" => anchor.wrapping_add(2),
                    _ => seed.initial_ea.unwrap(),
                };
                for target in 0..4u32 {
                    let source = seed.initial_ea.unwrap_or(seed.r.pc.wrapping_add(2));
                    let targets = [
                        logical,
                        anchor.wrapping_add(4),
                        source,
                        seed.operand_ea.unwrap_or(seed.r.pc.wrapping_add(4)),
                    ];
                    for failed in [false, true] {
                        for timing in ["before", "after"] {
                            let mutation = object(vec![
                                ("phase", s(phase)),
                                ("timing", s(timing)),
                                ("address", n(targets[target as usize])),
                                ("value", n(0x1234u32)),
                            ]);
                            emit(changed(
                                base,
                                vec![
                                    ("mutation", mutation),
                                    ("target", n(target)),
                                    ("fail_phase", optional(failed.then_some(phase))),
                                ],
                            ));
                        }
                    }
                }
            }
        }
        16 => {
            let seed = cfg(base);
            let anchor = seed.r.pc.wrapping_add(2 + 2 * seed.extensions as u32);
            for route in 0..6u32 {
                let fields = if route < 3 {
                    let address = [seed.r.pc, seed.r.pc.wrapping_add(2), anchor.wrapping_add(2)]
                        [route as usize]
                        .wrapping_add(base.get("lane").number() as u32);
                    if mode != 0 {
                        vec![("requested_EA", n(address))]
                    } else {
                        vec![("initial_RAM", list([list([n(address), n(0x1234u32)])]))]
                    }
                } else {
                    vec![(
                        "PC",
                        n([seed.r.ssp.wrapping_sub(6), 36, 0x4000][route as usize - 3]),
                    )]
                };
                let mut value = changed(base, fields);
                set(&mut value, "alias_route", n(route));
                set(
                    &mut value,
                    "fixed_words",
                    list(
                        [[36u32, 0], [38, 0x4000], [0x4000, 0x4e71], [0x4002, 0x4e71]]
                            .into_iter()
                            .map(|pair| list(pair.into_iter().map(n))),
                    ),
                );
                paths(&value, "F", emit);
            }
        }
        17 => {
            for phase in phases {
                for retained in [0x4e71, 0x4a45, 0xffff, 0x4afc, 0x4e72u32] {
                    for failed in [false, true] {
                        emit(changed(
                            base,
                            vec![
                                ("N", n(retained)),
                                ("selected_phase", s(phase)),
                                ("fail_phase", optional(failed.then_some(phase))),
                            ],
                        ));
                    }
                }
            }
        }
        18 => {
            for phase in phases {
                for room in [RESERVE - 1, RESERVE, RESERVE + 1] {
                    emit(changed(
                        base,
                        vec![("reserve_phase", s(phase)), ("room", n(room))],
                    ));
                }
            }
        }
        19 if matches!(mode, 4 | 6) => {
            for room in [0, 1, 2, 3, RESERVE - 1, RESERVE, RESERVE + 1] {
                emit(changed(
                    base,
                    vec![("start", n(u64::MAX - room)), ("prefix_room", n(room))],
                ));
            }
        }
        20 if size == 2 => {
            for room in [0, 1, 2, 3, RESERVE - 1, RESERVE, RESERVE + 1] {
                emit(changed(
                    base,
                    vec![("kind", s("Long_tail")), ("room", n(room))],
                ));
            }
        }
        21 | 22 => {
            let idle = if matches!(mode, 4 | 6) { 2 } else { 0 };
            for (index, phase) in phases.into_iter().enumerate() {
                let reports = if phase == "W" {
                    vec![(false, false), (true, false), (true, true)]
                } else {
                    vec![(false, false), (true, false)]
                };
                for (failed, deposit) in reports {
                    let waits = if section == 21 {
                        vec![0, 1, 2, 3]
                    } else {
                        vec![u32::MAX]
                    };
                    for wait in waits {
                        let mut value = changed(
                            base,
                            vec![
                                ("selected_phase", s(phase)),
                                ("fail_phase", optional(failed.then_some(phase))),
                                ("waits", object(vec![(phase, n(wait))])),
                                ("failed_write_deposit", b(deposit)),
                            ],
                        );
                        if section == 22 {
                            set(
                                &mut value,
                                "start",
                                n(u64::MAX - RESERVE - idle - index as u64 * 4),
                            );
                        }
                        emit(value);
                    }
                }
            }
        }
        23 => {
            if size == 2 {
                emit(changed(
                    base,
                    vec![("kind", s("Long_tail")), ("room", n(2u32))],
                ));
            } else {
                let phase = if mode != 0 { "W" } else { "F" };
                let idle = if matches!(mode, 4 | 6) { 2 } else { 0 };
                let prefix =
                    idle + phases.iter().position(|name| *name == phase).unwrap() as u64 * 4;
                emit(changed(
                    base,
                    vec![
                        ("start", n(u64::MAX - RESERVE - prefix)),
                        ("waits", object(vec![(phase, n(u32::MAX))])),
                    ],
                ));
            }
        }
        _ => {}
    }
}
