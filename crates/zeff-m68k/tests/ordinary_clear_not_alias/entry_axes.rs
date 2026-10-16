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
pub const SSP: [u32; 6] = [0, 2, 6, 0xfffffff0, 0x00fffff0, 0x80000010];
pub const HANDLERS: [u32; 6] = [0x4000, 0xfffffffc, 0xfffffffe, 0x00fffffc, 0x00fffffe, 0];

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
        assert_eq!(value.get("section").number(), 39);
        assert!(if trace {
            api == Api::Service
        } else {
            api == Api::Boundary
        });
        assert_eq!(value.get("trace").boolean(), trace);
        assert_eq!(value.get("IRQ").number(), 6);
        assert!(!value.get("IRQ_edge").boolean());
        assert_eq!(value.get("mask").number(), 0);
        assert_eq!(value.get("reply").text(), "Autovector");
        Self {
            primary: Axis::from_literal(value),
            value: value.clone(),
            section: 39,
            trace,
            api,
            irq: 6,
            edge: false,
            reply: Reply::parse(value.get("reply").text()),
        }
    }
}

fn fixed(handler: u32) -> Value {
    let mut words = Vec::new();
    for vector in [9u32, 27, 30, 31, 24, 64] {
        words.push(list([n(vector * 4), n(handler >> 16)]));
        words.push(list([n(vector * 4 + 2), n(handler & 65535)]));
    }
    words.push(list([n(handler), n(0x4e71u16)]));
    words.push(list([n(handler.wrapping_add(2)), n(0x4e71u16)]));
    list(words)
}

pub fn each(mut visit: impl FnMut(EntryAxis)) {
    let contexts = seed::contexts();
    let mut index = 0;
    for (context, base) in contexts.iter().enumerate() {
        let bits = [8u32, 16, 32][usize::from(base.word >> 6 & 3)];
        let seed = seed::build(base);
        for sp in SSP {
            for handler in HANDLERS {
                for value in [0u32, 1 << (bits - 1)] {
                    for wait in [0u32, 3] {
                        for trace in [true, false] {
                            let phases = if trace {
                                TRACE.as_slice()
                            } else {
                                IRQ.as_slice()
                            };
                            let fields = vec![
                                ("word", n(base.word)),
                                ("bank", n(base.bank)),
                                ("lane", n(base.lane)),
                                ("kind", s("nominal")),
                                ("entry", s(if trace { "trace" } else { "irq" })),
                                ("trace", b(trace)),
                                ("mask", n(0u8)),
                                (
                                    "api",
                                    s(if trace {
                                        "service_pending_trace"
                                    } else {
                                        "step_boundary"
                                    }),
                                ),
                                ("register_overrides", object(vec![("ssp", n(sp))])),
                                ("fixed_words", fixed(handler)),
                                ("value", n(value)),
                                (
                                    "waits",
                                    object(
                                        seed.device.phases[..seed.device.phase_count]
                                            .iter()
                                            .flatten()
                                            .map(|phase| (*phase, n(wait)))
                                            .collect(),
                                    ),
                                ),
                                (
                                    "entry_waits",
                                    object(phases.iter().map(|phase| (*phase, n(wait))).collect()),
                                ),
                                ("IRQ", n(6u8)),
                                ("IRQ_edge", b(false)),
                                ("reply", s("Autovector")),
                                ("requested_SSP", n(sp)),
                                ("requested_handler", n(handler)),
                                ("wait_variant", n(wait)),
                                (
                                    "original_nominal_id",
                                    Value::Owned(format!("OCN{context:04}")),
                                ),
                                ("id", Value::Owned(format!("OCEA39_{index:07}"))),
                                ("section", n(39u8)),
                            ];
                            visit(EntryAxis::from_value(&object(fields)));
                            index += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(index, 124416);
}
