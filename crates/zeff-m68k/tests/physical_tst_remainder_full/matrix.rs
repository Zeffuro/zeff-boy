use super::*;
use device::Kind;
use model::Spec;
pub(super) fn main(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    match index {
        4 | 5 => {
            for mut s in model::encoded() {
                for ccr in 0..32 {
                    for vi in 0..8 {
                        s.ccr = ccr;
                        s.vi(vi);
                        if index == 4 {
                            for waits in 0..2 {
                                tally.add(check::run(
                                    s,
                                    |b| {
                                        if waits == 1 {
                                            for (i, n) in [7, 11, 5, 13].into_iter().enumerate() {
                                                b.elapsed[i] = n;
                                            }
                                        }
                                    },
                                    mark,
                                ));
                            }
                        } else {
                            for at in 0..s.count() {
                                for (k, n) in model::REPORTS {
                                    tally.add(check::run(s, |b| b.report = Some((at, k, n)), mark));
                                }
                            }
                        }
                    }
                }
            }
        }
        6 => {
            for mut s in model::encoded().filter(|s| s.size() == 0) {
                for value in 0..256 {
                    s.value = value;
                    s.ccr = ((value & 1) * 16) as u16;
                    s.lane = value & 1;
                    s.desired = 0x1000 | s.lane;
                    tally.add(check::run(s, |_| {}, mark));
                }
            }
        }
        7 => {
            for bank in [false, true] {
                for value in 0..65536 {
                    let mut s = Spec::new(0x4a40, bank);
                    s.value = value;
                    s.ccr = ((value & 1) * 16) as u16;
                    tally.add(check::run(s, |_| {}, mark));
                }
            }
        }
        8 => {
            for mut s in model::encoded().filter(|s| s.size() == 2) {
                for ccr in 0..32 {
                    for hi in [0u32, 1, 0x8000, 0xffff] {
                        for lo in [0u32, 1, 0x8000, 0xffff] {
                            s.ccr = ccr;
                            s.value = hi << 16 | lo;
                            tally.add(check::run(s, |_| {}, mark));
                        }
                    }
                }
            }
        }
        9 => {
            for mut s in model::encoded().filter(|s| s.mode() == 0 && s.size() < 2) {
                for ccr in 0..32 {
                    for low in [0, 1, 1 << (s.bits() - 1), (1 << s.bits()) - 1] {
                        for upper in [0, 1, 1 << (31 - s.bits()), (1 << (32 - s.bits())) - 1] {
                            s.ccr = ccr;
                            s.value = low;
                            s.upper = upper;
                            tally.add(check::run(s, |_| {}, mark));
                        }
                    }
                }
            }
        }
        10 => {
            for bank in [false, true] {
                for word in [
                    0x4a00, 0x4a40, 0x4a80, 0x4a10, 0x4a18, 0x4a20, 0x4a28, 0x4a30, 0x4a38, 0x4a39,
                ] {
                    for mask in 0..8 {
                        for ccr in 0..32 {
                            let mut s = Spec::new(word, bank);
                            s.mask = mask;
                            s.ccr = ccr;
                            tally.add(check::run(s, |_| {}, mark));
                        }
                    }
                }
            }
        }
        11 => {
            for bank in [false, true] {
                for ext in 0..=u16::MAX {
                    let mut s = Spec::new(0x4a37, bank);
                    s.value = 0x80;
                    s.ext = Some(ext);
                    tally.add(check::run(s, |_| {}, mark));
                }
            }
        }
        12 => {
            for base in 0..8 {
                for bank in [false, true] {
                    for selector in 0..16 {
                        for width in 0..2 {
                            for ignored in 0..8 {
                                for offset in [0, 1, 0x80, 0xff] {
                                    for path in 0..4 {
                                        let mut s = Spec::new(0x4a30 | base, bank);
                                        s.value = 0x80;
                                        s.ext = Some(
                                            selector << 12 | width << 11 | ignored << 8 | offset,
                                        );
                                        tally.add(check::run(
                                            s,
                                            |b| {
                                                if path != 0 {
                                                    b.report = Some((
                                                        (path - 1) as usize,
                                                        Kind::Boundary,
                                                        8,
                                                    ));
                                                }
                                            },
                                            mark,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => unreachable!(),
    }
    tally.result()
}
