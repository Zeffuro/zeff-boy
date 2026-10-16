use crate::literal::Value;
use crate::seed::Axis;

pub const TRACE: [&str; 7] = [
    "TR_PCL", "TR_SR", "TR_PCH", "TR_VH", "TR_VL", "TR_H0", "TR_H1",
];
pub const EXTRA: [&str; 17] = [
    "ProbeFetch0",
    "ProbeFetch1",
    "TR_PCL",
    "TR_SR",
    "TR_PCH",
    "TR_VH",
    "TR_VL",
    "TR_H0",
    "TR_H1",
    "SetupPhysical",
    "SetupStopI4",
    "ResetSPHigh",
    "ResetSPLow",
    "ResetPCHigh",
    "ResetPCLow",
    "HandlerFirst",
    "HandlerSecond",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    PCMismatch,
    Unprimed,
    Stopped,
    PendingTrace,
    ImportedBoundary,
    HostFrozen,
    PhysicalUnqualified,
    DoubleHalt,
}
impl State {
    pub const ALL: [Self; 8] = [
        Self::PCMismatch,
        Self::Unprimed,
        Self::Stopped,
        Self::PendingTrace,
        Self::ImportedBoundary,
        Self::HostFrozen,
        Self::PhysicalUnqualified,
        Self::DoubleHalt,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::PCMismatch => "PCMismatch",
            Self::Unprimed => "Unprimed",
            Self::Stopped => "Stopped",
            Self::PendingTrace => "PendingTrace",
            Self::ImportedBoundary => "ImportedBoundary",
            Self::HostFrozen => "HostFrozen",
            Self::PhysicalUnqualified => "PhysicalUnqualified",
            Self::DoubleHalt => "DoubleHalt",
        }
    }
    pub fn parse(name: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|state| state.name() == name)
            .unwrap()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    Instruction,
    Boundary,
    Prime,
    Trace,
    Step,
}
impl Api {
    pub const ALL: [Self; 5] = [
        Self::Instruction,
        Self::Boundary,
        Self::Prime,
        Self::Trace,
        Self::Step,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Instruction => "step_instruction",
            Self::Boundary => "step_boundary",
            Self::Prime => "prime",
            Self::Trace => "service_pending_trace",
            Self::Step => "step",
        }
    }
    pub fn parse(name: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|api| api.name() == name)
            .unwrap()
    }
}

#[derive(Clone, Debug)]
pub struct PreflightAxis {
    pub value: Value,
    pub primary: Axis,
    pub state: State,
    pub api: Api,
}

pub fn object(fields: Vec<(&'static str, Value)>) -> Value {
    Value::Object(fields)
}
pub fn n(value: impl Into<u64>) -> Value {
    Value::Integer(value.into())
}
pub fn s(value: &'static str) -> Value {
    Value::Text(value)
}
pub fn list(values: impl IntoIterator<Item = Value>) -> Value {
    Value::List(values.into_iter().collect())
}

impl PreflightAxis {
    pub fn from_value(value: &Value) -> Self {
        let state = State::parse(value.get("state").text());
        let mut primary = Axis::new(
            value.get("word").number() as u16,
            value.get("bank").number() as u8,
            value.get("lane").number() as u8,
        );
        let Value::Object(fields) = value else {
            panic!("axis object");
        };
        let mut base = fields
            .iter()
            .filter(|(key, _)| {
                !["id", "section", "state", "api", "original_nominal_id"].contains(key)
            })
            .cloned()
            .collect::<Vec<_>>();
        base.push(("id", value.get("original_nominal_id").clone()));
        if state == State::HostFrozen {
            base.extend([
                ("fail_phase", s("F")),
                ("waits", object(vec![("F", n(3u8))])),
            ]);
        }
        if state == State::PendingTrace {
            base.push(("trace", Value::Bool(true)));
        }
        primary.source = Some(object(base));
        Self {
            value: value.clone(),
            primary,
            state,
            api: Api::parse(value.get("api").text()),
        }
    }
}

pub fn each(mut apply: impl FnMut(PreflightAxis)) {
    let fixed = [
        [36u32, 0],
        [38, 0x4000],
        [0x4000, 0x4e71],
        [0x4002, 0x4e71],
        [0, 9],
        [2, 0],
        [4, 0],
        [6, 0x7000],
        [0x7000, 0x4e71],
        [0x7002, 0x4e71],
    ];
    let mut context = 0;
    let mut count = 0;
    for operation in [0x4200u16, 0x4600] {
        for bank in 0..2u8 {
            for size in 0..3u16 {
                for mode in [0, 2, 3, 4, 5, 6, 7] {
                    if size != 0 && mode != 0 {
                        continue;
                    }
                    for register in 0..if mode == 7 { 2 } else { 8 } {
                        for lane in 0..if mode == 0 { 1 } else { 2u8 } {
                            for state in State::ALL {
                                for api in Api::ALL {
                                    let value = object(vec![
                                        ("word", n(operation | size << 6 | mode << 3 | register)),
                                        ("bank", n(bank)),
                                        ("lane", n(lane)),
                                        ("kind", s("nominal")),
                                        ("id", Value::Owned(format!("OCPF26_{count:05}"))),
                                        (
                                            "original_nominal_id",
                                            Value::Owned(format!("OCN{context:04}")),
                                        ),
                                        ("section", n(26u8)),
                                        ("state", s(state.name())),
                                        ("api", s(api.name())),
                                        ("fixed_words", list(fixed.map(|pair| list(pair.map(n))))),
                                    ]);
                                    apply(PreflightAxis::from_value(&value));
                                    count += 1;
                                }
                            }
                            context += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!((context, count), (432, 17280));
}
