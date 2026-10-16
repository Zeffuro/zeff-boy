use super::Registers;
use super::model::{Case, Profile};

pub(super) fn make(word: u16, supervisor: bool, lane: u32) -> Case {
    let p = Profile::decode(word).unwrap();
    assert!(lane < 2 && (p.memory() || lane == 0));
    let mut r = Registers::new();
    r.d = std::array::from_fn(|i| 0x1111_1111 * (i as u32 + 1));
    r.a = std::array::from_fn(|i| 0x3000 + i as u32 * 0x1000);
    r.usp = 0xb000;
    r.ssp = 0xa000;
    r.pc = 0x2000;
    r.set_status_register(if supervisor { 0x251f } else { 0x051f });
    let retained = match p.mode {
        5 => 16,
        6 => 0x3010,
        7 if p.register == 0 => 0x1000 + lane as u16,
        7 => 0,
        _ => 0x7001,
    };
    let low = if p.extensions() == 2 {
        0x1000 + lane as u16
    } else {
        0
    };
    if p.mode == 0 {
        r.d[p.register] = match p.bits {
            8 => 0xa5b6_c700,
            16 => 0xa5b6_0000,
            _ => 0,
        };
    } else {
        if p.mode == 6 {
            r.d[3] = 2;
        }
        if p.mode < 7 {
            let base = match p.mode {
                4 => 0x1000 + lane + p.step(),
                5 => 0x1000 + lane - 16,
                6 => 0x1000 + lane - 18,
                _ => 0x1000 + lane,
            };
            assert!(r.set_address_register(p.register, base));
        }
    }
    Case {
        word,
        p,
        r,
        retained,
        low,
        operand: if lane == 0 { 0x005a } else { 0x5a00 },
        next: 0x7001,
        last: 0x4e71,
        start: 0,
        handler: 0x4000,
        reset_handler: 0x7000,
        reset_ssp: 0x90000,
    }
}

pub(super) fn each(mut f: impl FnMut(Case)) {
    for word in 0..=u16::MAX {
        if let Some(p) = Profile::decode(word) {
            for supervisor in [false, true] {
                for lane in 0..if p.memory() { 2 } else { 1 } {
                    f(make(word, supervisor, lane));
                }
            }
        }
    }
}
