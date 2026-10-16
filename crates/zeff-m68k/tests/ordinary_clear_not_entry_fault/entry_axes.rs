use crate::binding::{b, list, n, object, s};
use crate::device::Reply;
use crate::literal::Value;
use crate::seed::{self, Axis, RESERVE};

pub const TRACE: [&str; 7] = [
    "TR_PCL", "TR_SR", "TR_PCH", "TR_VH", "TR_VL", "TR_H0", "TR_H1",
];
pub const IRQ: [&str; 8] = [
    "IQ_PCL", "IQ_ACK", "IQ_SR", "IQ_PCH", "IQ_VH", "IQ_VL", "IQ_H0", "IQ_H1",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    Instruction,
    Boundary,
    Step,
    Service,
}

#[derive(Clone, Debug)]
pub struct EntryAxis {
    pub primary: Axis,
    pub value: Value,
    pub section: u8,
    pub trace: bool,
    pub api: Api,
    pub irq: u8,
    pub edge: bool,
    pub reply: Reply,
}

impl EntryAxis {
    pub fn from_value(value: &Value) -> Self {
        let trace = value.get("entry").text() == "trace";
        let api = match value.get("api").text() {
            "step_instruction" => Api::Instruction,
            "step_boundary" => Api::Boundary,
            "step" => Api::Step,
            "service_pending_trace" => Api::Service,
            _ => panic!("API"),
        };
        let section = value.get("section").number() as u8;
        assert!((29..=32).contains(&section));
        assert!(if trace {
            matches!(api, Api::Service | Api::Step)
        } else {
            api == Api::Boundary
        });
        assert_eq!(value.get("trace").boolean(), trace);
        assert_eq!(value.get("mask").number(), 0);
        Self {
            primary: Axis::from_literal(value),
            value: value.clone(),
            section,
            trace,
            api,
            irq: value.get("IRQ").number() as u8,
            edge: value.get("IRQ_edge").boolean(),
            reply: Reply::parse(value.get("reply").text()),
        }
    }
}

fn fixed() -> Value {
    let mut words = Vec::new();
    for (index, vector) in [9u32, 27, 30, 31, 24, 64].into_iter().enumerate() {
        let handler = 0x4000 + index as u32 * 0x100;
        words.extend([
            list([n(vector * 4), n(0u16)]),
            list([n(vector * 4 + 2), n(handler)]),
            list([n(handler), n(0x4e71u16)]),
            list([n(handler + 2), n(0x4e71u16)]),
        ]);
    }
    list(words)
}

fn base(axis: &Axis, trace: bool) -> Vec<(&'static str, Value)> {
    vec![
        ("word", n(axis.word)),
        ("bank", n(axis.bank)),
        ("lane", n(axis.lane)),
        ("kind", s("nominal")),
        ("entry", s(if trace { "trace" } else { "irq" })),
        ("mask", n(0u8)),
        ("trace", b(trace)),
        ("fixed_words", fixed()),
        (
            "api",
            s(if trace {
                "service_pending_trace"
            } else {
                "step_boundary"
            }),
        ),
        ("IRQ", n(6u8)),
        ("IRQ_edge", b(false)),
        ("reply", s("Autovector")),
    ]
}

fn put(fields: &mut Vec<(&'static str, Value)>, key: &'static str, value: Value) {
    if let Some((_, old)) = fields.iter_mut().find(|(name, _)| *name == key) {
        *old = value;
    } else {
        fields.push((key, value));
    }
}

fn prefix(axis: &Axis, trace: bool, target: &str, waits: &Value, entry_waits: &Value) -> u64 {
    let seed = seed::build(axis);
    let mode = axis.word >> 3 & 7;
    let mut events = Vec::new();
    if matches!(mode, 4 | 6) {
        events.push(("InstructionI2", 2u64));
    }
    for phase in seed.device.phases[..seed.device.phase_count]
        .iter()
        .flatten()
    {
        events.push((*phase, 4 + waits.find(phase).map_or(0, Value::number)));
    }
    if axis.word >> 6 & 3 == 2 {
        events.push(("LongTailI2", 2));
    }
    if trace {
        events.push(("TraceI4", 4));
        for phase in TRACE {
            if phase == "TR_H1" {
                events.push(("TraceI2", 2));
            }
            events.push((phase, 4 + entry_waits.find(phase).map_or(0, Value::number)));
        }
    } else {
        events.push(("InterruptI6", 6));
        for phase in IRQ {
            if phase == "IQ_SR" {
                events.push(("InterruptI4", 4));
            }
            if phase == "IQ_H1" {
                events.push(("InterruptI2", 2));
            }
            events.push((phase, 4 + entry_waits.find(phase).map_or(0, Value::number)));
        }
    }
    let mut total = 0;
    for (phase, elapsed) in events {
        if phase == target {
            return total;
        }
        total += elapsed;
    }
    panic!("target")
}

fn emit(
    fields: Vec<(&'static str, Value)>,
    section: u8,
    context: usize,
    index: &mut usize,
    visit: &mut impl FnMut(EntryAxis),
) {
    let mut fields = fields;
    fields.extend([
        (
            "original_nominal_id",
            Value::Owned(format!("OCN{context:04}")),
        ),
        ("id", Value::Owned(format!("OCEF{section:02}_{index:07}"))),
        ("section", n(section)),
    ]);
    visit(EntryAxis::from_value(&object(fields)));
    *index += 1;
}

pub fn each(mut visit: impl FnMut(EntryAxis)) {
    let contexts = seed::contexts();
    for section in 29u8..=32 {
        let mut index = 0;
        for (context, axis) in contexts.iter().enumerate() {
            if matches!(section, 29 | 32) {
                let trace = section == 29;
                let names = if trace {
                    TRACE.as_slice()
                } else {
                    IRQ.as_slice()
                };
                let mut reports = Vec::new();
                for phase in names {
                    for failed in [false, true] {
                        reports.push((*phase, failed, false));
                    }
                }
                for phase in names {
                    if phase.ends_with("PCL") || phase.ends_with("SR") || phase.ends_with("PCH") {
                        reports.push((*phase, true, true));
                    }
                }
                for (phase, failed, deposited) in reports {
                    let configurations = if trace { 8 } else { 36 };
                    for config in 0..configurations {
                        let mut fields = base(axis, trace);
                        let waits = [0u32, 1, 3, u32::MAX];
                        let wait = if trace {
                            waits[config / 2]
                        } else {
                            waits[config % 4]
                        };
                        if trace {
                            put(
                                &mut fields,
                                "api",
                                s(if config % 2 == 0 {
                                    "service_pending_trace"
                                } else {
                                    "step"
                                }),
                            );
                        } else {
                            let level = [3u8, 6, 7][config / 12];
                            put(&mut fields, "IRQ", n(if level == 7 { 0 } else { level }));
                            put(&mut fields, "IRQ_edge", b(level == 7));
                            put(
                                &mut fields,
                                "reply",
                                s(["Autovector", "Spurious", "Vector64"][config / 4 % 3]),
                            );
                        }
                        fields.extend([
                            ("selected_entry_phase", s(phase)),
                            (
                                "entry_fail_phase",
                                if failed { s(phase) } else { Value::Null },
                            ),
                            ("entry_failed_write_deposit", b(deposited)),
                            ("entry_waits", object(vec![(phase, n(wait))])),
                        ]);
                        emit(fields, section, context, &mut index, &mut visit);
                    }
                }
            } else if section == 30 {
                for trace in [true, false] {
                    for target in if trace {
                        TRACE.as_slice()
                    } else {
                        IRQ.as_slice()
                    } {
                        for room in [RESERVE - 1, RESERVE, RESERVE + 1] {
                            let elapsed =
                                prefix(axis, trace, target, &object(vec![]), &object(vec![]));
                            let mut fields = base(axis, trace);
                            fields.extend([
                                ("start", n(u64::MAX - room - elapsed)),
                                ("selected_entry_phase", s(target)),
                                ("entry_reserve_phase", s(target)),
                                ("entry_room", n(room)),
                                ("source_prefix", n(elapsed)),
                            ]);
                            emit(fields, section, context, &mut index, &mut visit);
                        }
                    }
                }
            } else {
                for (target, duration) in [
                    ("TraceI4", 4u64),
                    ("TraceI2", 2),
                    ("InterruptI6", 6),
                    ("InterruptI4", 4),
                    ("InterruptI2", 2),
                ] {
                    let trace = target.starts_with("Trace");
                    let mut fields = base(axis, trace);
                    if !trace {
                        put(&mut fields, "IRQ", n(0u8));
                        put(&mut fields, "IRQ_edge", b(true));
                    }
                    let mut waits = Vec::new();
                    let mut entry_waits = Vec::new();
                    if matches!(target, "TraceI4" | "InterruptI6") {
                        let seed = seed::build(axis);
                        waits.push((
                            seed.device.phases[seed.device.phase_count - 1].unwrap(),
                            n(u32::MAX),
                        ));
                    } else {
                        entry_waits.push((
                            if target == "InterruptI4" {
                                "IQ_ACK"
                            } else if trace {
                                "TR_H0"
                            } else {
                                "IQ_H0"
                            },
                            n(u32::MAX),
                        ));
                    }
                    let waits = object(waits);
                    let entry_waits = object(entry_waits);
                    let elapsed = prefix(axis, trace, target, &waits, &entry_waits);
                    for room in [duration - 1, duration, duration + 1] {
                        let mut requested = fields.clone();
                        requested.extend([
                            ("start", n(u64::MAX - room - elapsed)),
                            ("waits", waits.clone()),
                            ("entry_waits", entry_waits.clone()),
                            ("entry_idle_phase", s(target)),
                            ("entry_room", n(room)),
                            ("source_prefix", n(elapsed)),
                        ]);
                        emit(requested, section, context, &mut index, &mut visit);
                    }
                }
            }
        }
        assert_eq!(
            index,
            [58752, 19440, 6480, 295488][usize::from(section - 29)]
        );
    }
}
