use crate::binding::{b, list, n, object, s};
use crate::literal::Value;
use crate::seed;

const PHASES: [&str; 6] = [
    "ResetSPHigh",
    "ResetSPLow",
    "ResetPCHigh",
    "ResetPCLow",
    "HandlerFirst",
    "HandlerSecond",
];
const REPORTS: [(&str, u32); 9] = [
    ("Complete", 0),
    ("Complete", 1),
    ("Complete", 3),
    ("Complete", 4),
    ("Complete", 5),
    ("Complete", 7),
    ("HostFailure", 6),
    ("CriticalAbortReleased", 8),
    ("TimingUnavailable", 5),
];
const DURATION_KEYS: [&str; 5] = ["0", "1", "2", "3", "4"];

fn words(sp: u32, handler: u32) -> Value {
    list(
        [
            [0, sp >> 16],
            [2, sp & 65535],
            [4, handler >> 16],
            [6, handler & 65535],
            [handler, 0x4e71],
            [handler.wrapping_add(2), 0x4e71],
        ]
        .map(|pair| list(pair.map(n))),
    )
}

fn detector(route: usize) -> Value {
    let (outcome, elapsed, omitted) = [
        ("TimingUnavailable", 0u32, true),
        ("TimingUnavailable", 5, false),
        ("HostFailure", 6, false),
        ("Complete", 0, false),
        ("CriticalAbortReleased", 8, false),
    ][route.saturating_sub(1)];
    object(vec![
        ("outcome", s(outcome)),
        ("elapsed", n(elapsed)),
        ("omitted", b(omitted)),
    ])
}

fn durations(previous: Option<usize>) -> Value {
    object(previous.map_or_else(Vec::new, |index| vec![(DURATION_KEYS[index], n(u32::MAX))]))
}

pub fn each(mut visit: impl FnMut(Value, Value)) {
    let contexts = seed::contexts();
    assert_eq!(contexts.len(), 432);
    for section in 33u8..=38 {
        let mut index = 0;
        for kind in ["Complete", "HostF", "BlockedF", "LongTail"] {
            for (context, base) in contexts.iter().enumerate() {
                if kind == "LongTail" && base.word >> 6 & 3 != 2 {
                    continue;
                }
                let mut fields = vec![
                    ("word", n(base.word)),
                    ("bank", n(base.bank)),
                    ("lane", n(base.lane)),
                    (
                        "kind",
                        s(if kind == "LongTail" {
                            "Long_tail"
                        } else {
                            "nominal"
                        }),
                    ),
                    ("origin_kind", s(kind)),
                    ("fixed_words", words(0x90000, 0x7000)),
                    (
                        "original_nominal_id",
                        Value::Owned(format!("OCN{context:04}")),
                    ),
                    ("id", Value::Null),
                    ("section", n(section)),
                ];
                match kind {
                    "HostF" => fields.extend([
                        ("fail_phase", s("F")),
                        ("waits", object(vec![("F", n(3u32))])),
                    ]),
                    "BlockedF" => {
                        fields.extend([("reserve_phase", s("F")), ("room", n(u32::MAX as u64 + 3))])
                    }
                    "LongTail" => fields.push(("room", n(1u64))),
                    _ => (),
                }
                let mut emit = |sp: u32, handler: u32, request: Value| {
                    let mut fields = fields.clone();
                    fields
                        .iter_mut()
                        .find(|(key, _)| *key == "fixed_words")
                        .unwrap()
                        .1 = words(sp, handler);
                    fields.iter_mut().find(|(key, _)| *key == "id").unwrap().1 =
                        Value::Owned(format!("OCR{section:02}_{index:07}"));
                    visit(object(fields), request);
                    index += 1;
                };
                match section {
                    33 => {
                        for at in 0usize..6 {
                            for (outcome, elapsed) in REPORTS {
                                emit(
                                    0x90000,
                                    0x7000,
                                    object(vec![(
                                        "report",
                                        object(vec![
                                            ("at", n(at as u64)),
                                            ("outcome", s(outcome)),
                                            ("elapsed", n(elapsed)),
                                        ]),
                                    )]),
                                );
                            }
                        }
                    }
                    34 => {
                        for sp in [0x90000, 0x90001] {
                            for route in 0usize..6 {
                                let handler = if route == 0 { 0x7000 } else { 0x7001 };
                                emit(
                                    sp,
                                    handler,
                                    object(vec![
                                        ("loaded_SP", n(sp)),
                                        ("handler_route", n(route as u64)),
                                        ("detector", detector(route)),
                                    ]),
                                );
                            }
                        }
                    }
                    35 => {
                        for route in 0u8..3 {
                            emit(
                                0x90000,
                                0x7000,
                                object(vec![
                                    ("qualified", b(route != 1)),
                                    ("nonmonotonic_release", b(route == 2)),
                                    ("rejection_route", n(route)),
                                ]),
                            );
                        }
                    }
                    36 => {
                        for (at, phase) in PHASES.into_iter().enumerate() {
                            let prefix = 14
                                + at as u64 * 4
                                + if at == 5 { 2 } else { 0 }
                                + if at == 0 { 0 } else { u32::MAX as u64 - 4 };
                            for room in [u32::MAX as u64 - 1, u32::MAX as u64, u32::MAX as u64 + 1]
                            {
                                emit(
                                    0x90000,
                                    0x7000,
                                    object(vec![
                                        ("release", Value::Decimal(u64::MAX - room - prefix)),
                                        ("durations", durations(at.checked_sub(1))),
                                        ("selected_reset_phase", s(phase)),
                                        ("reserve_room", n(room)),
                                        ("source_prefix", n(prefix)),
                                    ]),
                                );
                            }
                        }
                    }
                    37 => {
                        for (phase, elapsed) in [("ResetI14", 14u64), ("ResetI2", 2)] {
                            let late = phase == "ResetI2";
                            let prefix = if late {
                                14 + 5 * 4 + u32::MAX as u64 - 4
                            } else {
                                0
                            };
                            for room in [elapsed - 1, elapsed, elapsed + 1] {
                                emit(
                                    0x90000,
                                    0x7000,
                                    object(vec![
                                        ("release", Value::Decimal(u64::MAX - room - prefix)),
                                        ("durations", durations(late.then_some(4))),
                                        ("selected_reset_idle", s(phase)),
                                        ("idle_room", n(room)),
                                        ("source_prefix", n(prefix)),
                                    ]),
                                );
                            }
                        }
                    }
                    38 => {
                        for level in 0u8..8 {
                            emit(
                                0x90000,
                                0x7000,
                                object(vec![("level", n(level)), ("level7_edge", b(true))]),
                            );
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        assert_eq!(
            index,
            [71712, 15936, 3984, 23904, 7968, 10624][usize::from(section - 33)]
        );
    }
}
