use super::Registers;
use super::model::{Case, Profile};

pub(super) fn each(mut f: impl FnMut(Case)) {
    for word in 0xb000..=0xbfff {
        if let Some(p) = Profile::decode(word) {
            for bank in [false, true] {
                for lane in 0..if p.memory() { 2 } else { 1 } {
                    f(base(word, bank, if p.memory() { lane } else { 1 }));
                }
            }
        }
    }
}
pub(super) fn base(word: u16, bank: bool, lane: u32) -> Case {
    let p = Profile::decode(word).unwrap();
    let mut r = Registers::new();
    r.d = std::array::from_fn(|i| 0xaabb_cc00 + i as u32 * 2);
    r.a = std::array::from_fn(|i| 0x1234_3000 + i as u32 * 16);
    r.usp = 0x8000;
    r.ssp = 0x9000;
    r.pc = 0x1000;
    r.set_status_register(if bank { 0x2014 } else { 0x14 });
    let sign = 1u32 << (p.bits - 1);
    if p.cmpa {
        r.set_address_register(p.destination, 0x7fff_ffff);
    } else {
        r.d[p.destination] = trap(sign - 1, p.bits);
    }
    let mut c = Case {
        word,
        p,
        r,
        retained: 0x7201,
        low: 0,
        operand: if lane == 0 { 0x8055 } else { 0xaa80 },
        lane,
        next: 0x7201,
        last: 0x7480,
        start: 37,
    };
    if p.memory() {
        let target = 0x3000 + lane;
        match (p.mode, p.register) {
            (2 | 3, _) => {
                c.r.set_address_register(p.register, target);
            }
            (4, _) => {
                c.r.set_address_register(p.register, target + p.step());
            }
            (5, _) => {
                c.retained = 0xfff0;
                c.r.set_address_register(p.register, target + 16);
            }
            (6, _) => {
                c.retained = 0x28f0;
                c.r.d[2] = 0x20;
                c.r.set_address_register(p.register, target - 16);
            }
            (7, 0) => c.retained = target as u16,
            (7, 1) => {
                c.retained = 0;
                c.low = target as u16;
            }
            (7, 2) => c.retained = (target - 0x1002) as u16,
            (7, 3) => {
                c.retained = 0x28f0;
                c.r.d[2] = target - 0x1002 + 16;
            }
            _ => unreachable!(),
        }
    } else if p.immediate() {
        c.retained = if p.bits == 8 {
            0xff80
        } else if p.bits == 32 {
            (sign >> 16) as u16
        } else {
            sign as u16
        };
    } else if p.mode == 0 {
        if p.cmpa || p.register != p.destination {
            c.r.d[p.register] = trap(sign, p.bits);
        }
    } else if p.cmpa && p.register == p.destination {
        c.r.set_address_register(p.register, 0x8000_3000);
    } else {
        c.r.set_address_register(p.register, if p.bits == 16 { 0x8000_8000 } else { sign });
    }
    c
}
fn trap(value: u32, bits: u8) -> u32 {
    match bits {
        8 => 0xaabb_cc00 | value & 255,
        16 => 0xaabb_0000 | value & 65535,
        32 => value,
        _ => unreachable!(),
    }
}
pub(super) fn status(mut c: Case, ccr: u16) -> Case {
    c.r.set_status_register(c.r.status_register() & !31 | ccr);
    c
}
pub(super) fn pair(mut c: Case, pair: (u32, u32)) -> Case {
    let p = c.p;
    if p.mode == 0 {
        c.r.d[p.register] = trap(pair.0, p.bits);
    } else if p.mode == 1 {
        c.r.set_address_register(
            p.register,
            if p.bits == 16 {
                0x1357_0000 | pair.0 & 65535
            } else {
                pair.0
            },
        );
    }
    if p.cmpa {
        c.r.set_address_register(p.destination, pair.1);
    } else {
        c.r.d[p.destination] = trap(pair.1, p.bits);
    }
    if p.immediate() {
        c.retained = match p.bits {
            8 => 0x5a00 | pair.0 as u16 & 255,
            16 => pair.0 as u16,
            32 => (pair.0 >> 16) as u16,
            _ => unreachable!(),
        };
        c.low = pair.0 as u16;
    }
    if p.memory() {
        let target = 0x3000 + c.lane;
        if p.index() {
            let selector = (p.destination + 1) % 8;
            c.retained = (selector as u16) << 12 | 0x8f0;
            c.r.d[selector] = if p.mode == 7 {
                target - 0x1002 + 16
            } else {
                0x20
            };
            if p.mode == 6 {
                c.r.set_address_register(p.register, target - 16);
            }
        }
        c.operand = if c.lane == 0 {
            (pair.0 as u16 & 255) << 8 | 0x5a
        } else {
            0x5a00 | pair.0 as u16 & 255
        };
    }
    c
}
pub(super) fn pairs(c: Case) -> ([(u32, u32); 12], usize) {
    let p = c.p;
    let sign = 1u32 << (p.bits - 1);
    let mask = if p.bits == 32 {
        u32::MAX
    } else {
        (1u32 << p.bits) - 1
    };
    if !p.cmpa {
        return (
            [
                (0, 0),
                (0, 1),
                (1, 0),
                (sign - 1, sign),
                (sign, sign - 1),
                (sign, sign),
                (mask, sign - 1),
                (sign + 1, sign),
                (0, sign),
                (if p.bits == 32 { 0x10000 } else { sign - 1 }, 0),
                (0, 0),
                (0, 0),
            ],
            10,
        );
    }
    if p.bits == 16 {
        (
            [
                (0, 0),
                (0, 2),
                (1, 0),
                (2, 2),
                (0x7fff, 0x8000_0000),
                (0x8000, 0x7fff_fffe),
                (0x8000, 0xffff_8000),
                (0xffff, 0x7fff_fffe),
                (0, 0x8000_0000),
                (0x8001, 0),
                (0x7ffe, 0),
                (0x7fff, 0),
            ],
            12,
        )
    } else {
        (
            [
                (0, 0),
                (0, 2),
                (1, 0),
                (2, 2),
                (0x7fff_fffe, 0x8000_0000),
                (0x8000_0000, 0x7fff_fffe),
                (0x8000_0000, 0x8000_0000),
                (0xffff_ffff, 0x7fff_fffe),
                (0, 0x8000_0000),
                (0x8000_0001, 0),
                (0x10000, 0),
                (0x7fff_ffff, 0),
            ],
            12,
        )
    }
}
