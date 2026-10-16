use crate::device::{Device, Mutation};
use crate::literal::Value;
use zeff_m68k::Registers;

pub const RESERVE: u64 = u32::MAX as u64 + 4;
pub const LIMIT: u64 = u64::MAX - RESERVE;

#[derive(Clone, Debug)]
pub struct Axis {
    pub word: u16,
    pub bank: u8,
    pub lane: u8,
    pub section: &'static str,
    pub phase: Option<&'static str>,
    pub room: u64,
    pub source: Option<Value>,
}

impl Axis {
    pub fn new(word: u16, bank: u8, lane: u8) -> Self {
        Self {
            word,
            bank,
            lane,
            section: "default_completion",
            phase: None,
            room: 0,
            source: None,
        }
    }

    pub fn from_literal(value: &Value) -> Self {
        Self {
            word: value.get("word").number() as u16,
            bank: value.get("bank").number() as u8,
            lane: value.find("lane").map_or(0, |v| v.number() as u8),
            section: value.get("section").text(),
            phase: value
                .find("reserve_phase")
                .or_else(|| value.find("fail_phase"))
                .map(Value::text),
            room: value.find("room").map_or(0, Value::number),
            source: Some(value.clone()),
        }
    }

    pub fn find(&self, key: &str) -> Option<&Value> {
        self.source.as_ref().and_then(|value| value.find(key))
    }

    pub fn number(&self, key: &str, default: u64) -> u64 {
        self.find(key).map_or(default, Value::number)
    }
}

#[derive(Clone, Debug)]
pub struct Seed {
    pub axis: Axis,
    pub r: Registers,
    pub queue: [u16; 2],
    pub device: Device,
    pub bits: u32,
    pub mode: u16,
    pub register: usize,
    pub extensions: usize,
    pub requested: Option<u32>,
    pub operand_ea: Option<u32>,
    pub initial_ea: Option<u32>,
    pub level: u8,
    pub edge: bool,
}

pub fn active(r: &Registers, reg: usize) -> u32 {
    if reg < 7 {
        r.a[reg]
    } else if r.status_register() & 0x2000 != 0 {
        r.ssp
    } else {
        r.usp
    }
}

pub fn set_active(r: &mut Registers, reg: usize, value: u32) {
    if reg < 7 {
        r.a[reg] = value;
    } else if r.status_register() & 0x2000 != 0 {
        r.ssp = value;
    } else {
        r.usp = value;
    }
}

pub fn indexed(r: &Registers, base: u32, ext: u16) -> u32 {
    let reg = usize::from(ext >> 12 & 7);
    let index = if ext & 0x8000 != 0 {
        active(r, reg)
    } else {
        r.d[reg]
    };
    let index = if ext & 0x0800 == 0 {
        index as u16 as i16 as i32 as u32
    } else {
        index
    };
    base.wrapping_add(index)
        .wrapping_add_signed(i32::from(ext as u8 as i8))
}

fn overrides(r: &mut Registers, value: Option<&Value>) {
    let Some(Value::Object(fields)) = value else {
        return;
    };
    for (key, value) in fields {
        match *key {
            "d" => {
                assert_eq!(value.list().len(), 8);
                for (slot, value) in r.d.iter_mut().zip(value.list()) {
                    *slot = value.number() as u32;
                }
            }
            "a" => {
                assert_eq!(value.list().len(), 7);
                for (slot, value) in r.a.iter_mut().zip(value.list()) {
                    *slot = value.number() as u32;
                }
            }
            "pc" => r.pc = value.number() as u32,
            "ssp" => r.ssp = value.number() as u32,
            "usp" => r.usp = value.number() as u32,
            "sr" => r.set_status_register(value.number() as u16 & 0xa71f),
            _ => panic!("register override"),
        }
    }
}

fn projected(r: &Registers, mode: u16, reg: usize, retained: u16, device: &Device) -> Option<u32> {
    let base = active(r, reg);
    Some(match mode {
        0 => return None,
        2 | 3 => base,
        4 => base.wrapping_sub(if reg == 7 { 2 } else { 1 }),
        5 => base.wrapping_add_signed(i32::from(retained as i16)),
        6 => indexed(r, base, retained),
        7 if reg == 0 => retained as i16 as i32 as u32,
        7 => u32::from(retained) << 16 | u32::from(device.words.sample(r.pc.wrapping_add(4))),
        _ => panic!("mode"),
    })
}

pub fn build(axis: &Axis) -> Seed {
    let size = axis.word >> 6 & 3;
    let mode = axis.word >> 3 & 7;
    let reg = usize::from(axis.word & 7);
    assert!(matches!(axis.word & 0xff00, 0x4200 | 0x4600) && size < 3);
    assert!(mode == 0 || size == 0 && ((2..=6).contains(&mode) || mode == 7 && reg < 2));
    let bits = [8, 16, 32][usize::from(size)];
    let mask = if bits == 32 {
        u32::MAX
    } else {
        (1 << bits) - 1
    };
    let value = axis.number(
        "value",
        if bits == 32 {
            0x80008000
        } else {
            1 << (bits - 1)
        },
    ) as u32
        & mask;
    let mut r = Registers::new();
    for (i, d) in r.d.iter_mut().enumerate() {
        *d = 0x11111111 * (i as u32 + 1);
    }
    for (i, a) in r.a.iter_mut().enumerate() {
        *a = 0x3000 + 4096 * i as u32;
    }
    r.usp = 0x6000;
    r.ssp = 0x8000;
    r.pc = axis.number("PC", 0x2000) as u32;
    r.set_status_register(if axis.bank == 1 { 0x251f } else { 0x051f });
    overrides(&mut r, axis.find("initial_registers"));
    if let Some(sr) = axis.find("sr") {
        r.set_status_register(sr.number() as u16 & 0xa71f);
    } else if ["CCR", "mask", "trace"]
        .iter()
        .any(|key| axis.find(key).is_some())
    {
        let sr = r.status_register();
        let trace = axis.find("trace").map_or(sr & 0x8000 != 0, Value::boolean);
        r.set_status_register(
            sr & 0x2000
                | axis.number("CCR", u64::from(sr)) as u16 & 31
                | (axis.number("mask", u64::from(sr >> 8 & 7)) as u16 & 7) << 8
                | if trace { 0x8000 } else { 0 },
        );
    }
    let requested =
        (mode != 0).then(|| axis.number("requested_EA", 0x1000 + u64::from(axis.lane)) as u32);
    if mode == 0 {
        r.d[reg] =
            axis.number("upper", if bits < 32 { 0xdead0000 } else { 0 }) as u32 & !mask | value;
    }
    if mode == 6
        && axis
            .find("initial_registers")
            .and_then(|v| v.find("d"))
            .is_none()
    {
        r.d[3] = 2;
    }
    let extension_count = if (mode, reg) == (7, 1) {
        2
    } else {
        usize::from(mode >= 5)
    };
    let mut ext = [0; 2];
    if let Some(values) = axis.find("extensions") {
        assert_eq!(values.list().len(), extension_count);
        for (slot, value) in ext.iter_mut().zip(values.list()) {
            *slot = value.number() as u16;
        }
    } else {
        match (mode, reg) {
            (5, _) => ext[0] = 16,
            (6, _) => ext[0] = 0x3010,
            (7, 0) => ext[0] = requested.unwrap() as u16,
            (7, 1) => {
                ext[0] = (requested.unwrap() >> 16) as u16;
                ext[1] = requested.unwrap() as u16;
            }
            _ => {}
        }
    }
    if (2..=6).contains(&mode) && !axis.find("preserve_base").is_some_and(Value::boolean) {
        let requested = requested.unwrap();
        let base = match mode {
            4 => requested.wrapping_add(if reg == 7 { 2 } else { 1 }),
            5 => requested.wrapping_sub(ext[0] as i16 as i32 as u32),
            6 => requested.wrapping_sub(indexed(&r, 0, ext[0])),
            _ => requested,
        };
        set_active(&mut r, reg, base);
    }
    overrides(&mut r, axis.find("register_overrides"));
    let anchor = r.pc.wrapping_add(2 + extension_count as u32 * 2);
    let n = axis.number("N", 0x7001) as u16;
    let f = axis.number("F", 0x4e71) as u16;
    let queue = [
        axis.word,
        axis.number(
            "retained",
            u64::from(if extension_count > 0 { ext[0] } else { n }),
        ) as u16,
    ];
    let mut device = Device::new(0);
    for (address, value) in [(r.pc, axis.word), (anchor, n), (anchor.wrapping_add(2), f)] {
        device.words.insert(address, value);
    }
    for (i, value) in ext[..extension_count].iter().enumerate() {
        device
            .words
            .insert(r.pc.wrapping_add(2 + 2 * i as u32), *value);
    }
    if let Some(values) = axis.find("fixed_words") {
        for pair in values.list() {
            device.words.insert(
                pair.list()[0].number() as u32,
                pair.list()[1].number() as u16,
            );
        }
    }
    let operand_ea = projected(&r, mode, reg, queue[1], &device);
    if let Some(ea) = operand_ea {
        let other = axis.number("other_byte", 0x5a) as u8;
        device.words.insert(
            ea,
            if ea & 1 == 0 {
                (value as u16) << 8 | u16::from(other)
            } else {
                u16::from(other) << 8 | value as u16
            },
        );
    }
    if let Some(values) = axis.find("initial_RAM") {
        for pair in values.list() {
            device.words.insert(
                pair.list()[0].number() as u32,
                pair.list()[1].number() as u16,
            );
        }
    }
    let initial_ea = projected(&r, mode, reg, queue[1], &device);
    let mut names = Vec::new();
    if extension_count == 2 {
        names.extend(["E1", "E2"]);
    } else if extension_count > 0 {
        names.push("E");
    }
    if mode != 0 {
        names.push("RH");
    }
    names.push("F");
    if mode != 0 {
        names.push("W");
    }
    device.phase_count = names.len();
    for (index, name) in names.iter().enumerate() {
        device.phases[index] = Some(name);
        device.waits[index] = axis
            .find("waits")
            .and_then(|v| v.find(name))
            .map_or(0, |v| v.number() as u32);
        if axis.section == "callback_Host" && axis.phase == Some(name) {
            device.waits[index] = 3;
        }
        if axis.section == "Long_tail" && *name == "F" {
            device.waits[index] = u32::MAX;
        }
    }
    device.fail = axis
        .find("fail_phase")
        .map(Value::text)
        .or(if axis.section == "callback_Host" {
            axis.phase
        } else {
            None
        });
    device.failed_write_deposit = axis
        .find("failed_write_deposit")
        .is_some_and(Value::boolean);
    let idle = if matches!(mode, 4 | 6) { 2 } else { 0 };
    let kind = axis.find("kind").map(Value::text).unwrap_or("nominal");
    let prefix = |phase: &str| {
        idle + names.iter().position(|name| *name == phase).unwrap() as u64 * 4
            + names
                .iter()
                .take_while(|name| **name != phase)
                .enumerate()
                .map(|(i, _)| u64::from(device.waits[i]))
                .sum::<u64>()
    };
    device.clock = if matches!(kind, "Dn_reserve" | "memory_reserve") {
        u64::MAX - axis.room - prefix("F")
    } else if axis.section == "blocked_transfer" || axis.find("reserve_phase").is_some() {
        u64::MAX - axis.room - prefix(axis.phase.unwrap())
    } else if axis.section == "Long_tail" || kind == "Long_tail" || axis.find("tail_room").is_some()
    {
        let f_index = names.iter().position(|name| *name == "F").unwrap();
        device.waits[f_index] = u32::MAX;
        LIMIT - axis.number("tail_room", axis.room)
    } else {
        axis.number("start", 0)
    };
    device.idle_phase = if mode == 0 {
        "LongTailI2"
    } else {
        "InstructionI2"
    };
    device.mutation = axis.find("mutation").map(|value| Mutation {
        phase: value.get("phase").text(),
        after: value.get("timing").text() == "after",
        address: value.get("address").number() as u32,
        value: value.get("value").number() as u16,
    });
    Seed {
        axis: axis.clone(),
        r,
        queue,
        device,
        bits,
        mode,
        register: reg,
        extensions: extension_count,
        requested,
        operand_ea,
        initial_ea,
        level: axis.number("interrupt_level", 0) as u8,
        edge: axis.find("level7_edge").is_some_and(Value::boolean),
    }
}

pub fn axes() -> Vec<Axis> {
    let mut contexts = Vec::new();
    for operation in [0x4200, 0x4600] {
        for bank in 0..2 {
            for size in 0..3 {
                for mode in [0, 2, 3, 4, 5, 6, 7] {
                    if size > 0 && mode > 0 {
                        continue;
                    }
                    for register in 0..if mode == 7 { 2 } else { 8 } {
                        for lane in 0..if mode == 0 { 1 } else { 2 } {
                            contexts.push(Axis::new(
                                operation | size << 6 | mode << 3 | register,
                                bank,
                                lane,
                            ));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(contexts.len(), 432);
    let mut rows = contexts.clone();
    for section in ["callback_Host", "blocked_transfer"] {
        for base in &contexts {
            let seed = build(base);
            for phase in seed.device.phases[..seed.device.phase_count]
                .iter()
                .flatten()
            {
                let mut axis = base.clone();
                axis.section = section;
                axis.phase = Some(phase);
                axis.room = if section == "blocked_transfer" {
                    RESERVE - 1
                } else {
                    0
                };
                rows.push(axis);
            }
        }
    }
    for base in &contexts {
        if base.word >> 6 & 3 != 2 {
            continue;
        }
        for room in 0..3 {
            let mut axis = base.clone();
            axis.section = "Long_tail";
            axis.room = room;
            rows.push(axis);
        }
    }
    assert_eq!(rows.len(), 3040);
    rows
}
