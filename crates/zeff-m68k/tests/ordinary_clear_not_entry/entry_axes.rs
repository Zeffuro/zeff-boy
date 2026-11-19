use crate::binding::{b, list, n, object, s};
use crate::device::Reply;
use crate::literal::Value;
use crate::seed::{self, Axis};

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
        Self {
            primary: Axis::from_literal(value),
            value: value.clone(),
            section: value.get("section").number() as u8,
            trace,
            api: match value.get("api").text() {
                "step_instruction" => Api::Instruction,
                "step_boundary" => Api::Boundary,
                "step" => Api::Step,
                _ => panic!("API"),
            },
            irq: value.find("IRQ").map_or(0, |v| v.number() as u8),
            edge: value.find("IRQ_edge").is_some_and(Value::boolean),
            reply: value
                .find("reply")
                .map_or(Reply::Autovector, |v| Reply::parse(v.text())),
        }
    }
}

fn fixed() -> Value {
    let mut words = Vec::new();
    for (i, vector) in [9u32, 27, 30, 31, 24, 64].into_iter().enumerate() {
        let handler = 0x4000 + i as u32 * 256;
        for (address, value) in [
            (vector * 4, 0),
            (vector * 4 + 2, handler),
            (handler, 0x4e71),
            (handler + 2, 0x4e71),
        ] {
            words.push(list([n(address), n(value)]));
        }
    }
    list(words)
}

fn common(
    base: &Axis,
    context: usize,
    section: u8,
    index: usize,
    mask: u8,
) -> Vec<(&'static str, Value)> {
    vec![
        ("word", n(base.word)),
        ("bank", n(base.bank)),
        ("lane", n(base.lane)),
        ("kind", s("nominal")),
        ("id", Value::Owned(format!("OCE{section:02}_{index:07}"))),
        (
            "original_nominal_id",
            Value::Owned(format!("OCN{context:04}")),
        ),
        ("section", n(section)),
        ("mask", n(mask)),
        ("fixed_words", fixed()),
    ]
}

pub fn each(mut visit: impl FnMut(EntryAxis)) {
    let contexts = seed::contexts();
    for section in [27u8, 28] {
        let mut index = 0;
        for (context, base) in contexts.iter().enumerate() {
            for mask in 0..8 {
                if section == 27 {
                    for route in 0..4 {
                        for reply in [
                            Reply::Unsupported,
                            Reply::Autovector,
                            Reply::Spurious,
                            Reply::Vector64,
                            Reply::Host,
                        ] {
                            for wait in [0u32, 1, 3, u32::MAX] {
                                let mut fields = common(base, context, section, index, mask);
                                fields.extend([
                                    ("entry", s("irq")),
                                    ("N", n(0x4e71u16)),
                                    ("IRQ", n([0u8, 3, 6, 0][route])),
                                    ("IRQ_edge", b(route == 3)),
                                    ("reply", s(reply.name())),
                                    ("entry_waits", object(vec![("IQ_ACK", n(wait))])),
                                    ("api", s("step_boundary")),
                                ]);
                                visit(EntryAxis::from_value(&object(fields)));
                                index += 1;
                            }
                        }
                    }
                } else {
                    let seed = seed::build(base);
                    for api in ["step_instruction", "step_boundary", "step"] {
                        for variant in 0..2 {
                            let mut fields = common(base, context, section, index, mask);
                            let waits = if variant == 0 {
                                Vec::new()
                            } else {
                                seed.device.phases[..seed.device.phase_count]
                                    .iter()
                                    .flatten()
                                    .copied()
                                    .zip([3u32, 7, 1, 9, 5])
                                    .map(|(phase, wait)| (phase, n(wait)))
                                    .collect()
                            };
                            let entry = if variant == 0 {
                                Vec::new()
                            } else {
                                TRACE
                                    .into_iter()
                                    .zip([3u32, 7, 1, 9, 5, 11, 13])
                                    .map(|(phase, wait)| (phase, n(wait)))
                                    .collect()
                            };
                            fields.extend([
                                ("entry", s("trace")),
                                ("trace", b(true)),
                                ("api", s(api)),
                                ("waits", object(waits)),
                                ("entry_waits", object(entry)),
                                ("wait_variant", n(variant as u8)),
                            ]);
                            visit(EntryAxis::from_value(&object(fields)));
                            index += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(index, if section == 27 { 276480 } else { 20736 });
    }
}
