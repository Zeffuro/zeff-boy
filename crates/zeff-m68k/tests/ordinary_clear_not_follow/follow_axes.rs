use crate::binding::{b, list, n, object, s};
use crate::literal::Value;
use crate::seed::{self, Axis};

pub const FRAME: [&str; 7] = [
    "NX_PCL", "NX_SR", "NX_PCH", "NX_VH", "NX_VL", "NX_H0", "NX_H1",
];
pub const NEXT: [u16; 5] = [0x4e71, 0x4a45, 0xffff, 0x4afc, 0x4e72];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    Instruction,
    Boundary,
    Step,
}

#[derive(Clone, Debug)]
pub struct FollowAxis {
    pub value: Value,
    pub primary: Axis,
    pub api: Api,
    pub next: u16,
}

impl FollowAxis {
    pub fn from_value(value: &Value) -> Self {
        let mut imported = value.clone();
        set(&mut imported, "section", s("default_completion"));
        let mut primary = Axis::from_literal(&imported);
        primary.source = Some(value.clone());
        Self {
            value: value.clone(),
            primary,
            api: match value.get("next_api").text() {
                "step_instruction" => Api::Instruction,
                "step_boundary" => Api::Boundary,
                "step" => Api::Step,
                _ => panic!("next API"),
            },
            next: value.get("N").number() as u16,
        }
    }
}

pub fn set(value: &mut Value, key: &'static str, new: Value) {
    let Value::Object(fields) = value else {
        panic!("object");
    };
    *fields.iter_mut().find(|(name, _)| *name == key).unwrap() = (key, new);
}

pub fn each(mut apply: impl FnMut(FollowAxis)) {
    let contexts = seed::contexts();
    assert_eq!(contexts.len(), 432);
    let mut count = 0;
    let mut fixed = Vec::new();
    for vector in [4u32, 8, 11] {
        let handler = 0x4000 + vector * 0x100;
        fixed.extend([
            [vector * 4, 0],
            [vector * 4 + 2, handler],
            [handler, 0x4e71],
            [handler + 2, 0x4e71],
        ]);
    }
    for (index, base) in contexts.into_iter().enumerate() {
        for next in NEXT {
            for api in ["step_instruction", "step_boundary", "step"] {
                let value = object(vec![
                    ("word", n(base.word)),
                    ("bank", n(base.bank)),
                    ("lane", n(base.lane)),
                    ("kind", s("nominal")),
                    ("id", Value::Owned(format!("OCON40_{count:07}"))),
                    (
                        "original_nominal_id",
                        Value::Owned(format!("OCN{index:04}")),
                    ),
                    ("section", n(40u8)),
                    ("N", n(next)),
                    ("next_api", s(api)),
                    (
                        "fixed_words",
                        list(fixed.iter().map(|pair| list(pair.map(n)))),
                    ),
                    ("trace", b(false)),
                    ("entry", s("trace")),
                ]);
                apply(FollowAxis::from_value(&value));
                count += 1;
            }
        }
    }
    assert_eq!(count, 6480);
}
