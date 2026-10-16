use crate::literal::Value;
use crate::seed::{self, Axis, RESERVE, Seed};

pub const COUNTS: [usize; 26] = [
    1769472, 1286144, 80384, 40192, 94208, 16384, 32768, 480, 2097152, 163840, 2097152, 262144,
    1536, 7776, 8064, 20096, 7776, 12560, 3768, 896, 224, 11392, 2848, 432, 344064, 1296,
];
pub const VALUES: [[u32; 8]; 3] = [
    [0, 1, 127, 128, 255, 85, 170, 64],
    [0, 1, 32767, 32768, 65535, 21845, 43690, 16384],
    [
        0, 1, 65536, 32768, 2147483648, 4294967295, 1431655765, 2863311530,
    ],
];
pub const WAITS: [[u32; 5]; 2] = [[3, 7, 1, 9, 5], [11, 5, 13, 3, 7]];

pub fn n(value: impl Into<u64>) -> Value {
    Value::Integer(value.into())
}
pub fn s(value: &'static str) -> Value {
    Value::Text(value)
}
pub fn b(value: bool) -> Value {
    Value::Bool(value)
}
pub fn object(fields: Vec<(&'static str, Value)>) -> Value {
    Value::Object(fields)
}
pub fn list(values: impl IntoIterator<Item = Value>) -> Value {
    Value::List(values.into_iter().collect())
}
pub fn optional(value: Option<&'static str>) -> Value {
    value.map_or(Value::Null, s)
}

pub fn set(value: &mut Value, key: &'static str, entry: Value) {
    let Value::Object(fields) = value else {
        panic!("axis object");
    };
    if let Some((_, old)) = fields.iter_mut().find(|(name, _)| *name == key) {
        *old = entry;
    } else {
        fields.push((key, entry));
    }
}

pub fn changed(base: &Value, fields: Vec<(&'static str, Value)>) -> Value {
    let mut value = base.clone();
    for (key, field) in fields {
        set(&mut value, key, field);
    }
    value
}

pub fn contexts() -> Vec<Value> {
    let mut rows = Vec::new();
    for operation in [0x4200u16, 0x4600] {
        for bank in 0..2u8 {
            for size in 0..3u16 {
                for mode in [0, 2, 3, 4, 5, 6, 7] {
                    if size != 0 && mode != 0 {
                        continue;
                    }
                    for reg in 0..if mode == 7 { 2 } else { 8 } {
                        for lane in 0..if mode == 0 { 1 } else { 2u8 } {
                            let index = rows.len();
                            rows.push(object(vec![
                                ("word", n(operation | size << 6 | mode << 3 | reg)),
                                ("bank", n(bank)),
                                ("lane", n(lane)),
                                ("kind", s("nominal")),
                                ("id", Value::Owned(format!("OCN{index:04}"))),
                            ]));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(rows.len(), 432);
    rows
}

pub fn geometry(value: &Value) -> (usize, u16, u16) {
    let word = value.get("word").number() as u16;
    (usize::from(word >> 6 & 3), word >> 3 & 7, word & 7)
}

pub fn names(value: &Value) -> Vec<&'static str> {
    let (_, mode, reg) = geometry(value);
    let mut names = if (mode, reg) == (7, 1) {
        vec!["E1", "E2"]
    } else if mode >= 5 {
        vec!["E"]
    } else {
        vec![]
    };
    if mode != 0 {
        names.push("RH");
    }
    names.push("F");
    if mode != 0 {
        names.push("W");
    }
    names
}

pub fn positional(base: &Value, variant: usize) -> Value {
    object(
        names(base)
            .iter()
            .zip(WAITS[variant])
            .map(|(phase, wait)| (*phase, n(wait)))
            .collect(),
    )
}

pub fn cfg(value: &Value) -> Seed {
    seed::build(&Axis::from_literal(&identified(value.clone(), 0, 0)))
}

pub fn identified(mut value: Value, section: u8, index: usize) -> Value {
    let original = value.get("id").clone();
    set(&mut value, "original_nominal_id", original);
    set(
        &mut value,
        "id",
        Value::Owned(format!("OCO{section:02}_{index:07}")),
    );
    set(&mut value, "section", n(section));
    value
}

pub fn paths(base: &Value, phase: &'static str, emit: &mut impl FnMut(Value)) {
    emit(changed(base, vec![("path", s("Complete"))]));
    emit(changed(
        base,
        vec![
            ("path", s("Host")),
            ("fail_phase", s(phase)),
            ("waits", object(vec![(phase, n(3u32))])),
        ],
    ));
    emit(changed(
        base,
        vec![
            ("path", s("Blocked")),
            ("reserve_phase", s(phase)),
            ("room", n(RESERVE - 1)),
        ],
    ));
}

pub fn each(section: u8, mut apply: impl FnMut(Value)) {
    assert!(section <= 25);
    let mut count = 0;
    for base in contexts() {
        let mut emit = |value| {
            apply(identified(value, section, count));
            count += 1;
        };
        if section <= 6 || matches!(section, 24 | 25) {
            crate::matrix::rows(section, &base, &mut emit);
        } else if matches!(section, 8..=12) {
            crate::extensions::rows(section, &base, &mut emit);
        } else {
            crate::directed::rows(section, &base, &mut emit);
        }
    }
    assert_eq!(count, COUNTS[usize::from(section)]);
}
