use super::*;
use model::{Spec, bank, forms, pairs, status};
use oracle::Count;

pub(super) fn indexes() -> (u32, u32) {
    let mut total = Count::default();
    for source in 0..8 {
        for selector in 0..16 {
            for long_index in [false, true] {
                for ignored in 0..8 {
                    for displacement in [0x10, 0xf0] {
                        for negative in [false, true] {
                            for size in [2, 4] {
                                for supervisor in [false, true] {
                                    for pair in [3, 4] {
                                        let mut s = Spec::new(size, 6, source, supervisor);
                                        let raw = match (long_index, negative) {
                                            (false, false) => 0x1234_0002,
                                            (false, true) => 0x1234_fffe,
                                            (true, false) => 2,
                                            (true, true) => 0xffff_fffe,
                                        };
                                        if selector < 8 {
                                            s.r.d[selector] = raw;
                                        } else {
                                            model::set_active(&mut s.r, selector - 8, raw);
                                        }
                                        s.extension[0] = ((selector as u16) << 12)
                                            | (u16::from(long_index) * 0x0800)
                                            | (ignored as u16) << 8
                                            | displacement;
                                        (s.operand, s.immediate) = pairs(size)[pair];
                                        let requested = model::address(s, s.extension);
                                        let (s, bus) = s.seed();
                                        let p = plan::validate(s, &bus);
                                        assert_eq!(p.address, requested);
                                        for host in [false, true] {
                                            let mut b = bus.clone();
                                            if host {
                                                b.fail = Some(p.final_read);
                                            }
                                            total.case(s, b);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn overlap() -> (u32, u32) {
    let mut total = Count::default();
    for form in forms() {
        for supervisor in [false, true] {
            for location in 0..3 {
                let mut s = bank(form, supervisor, 16);
                let target = match location {
                    0 => s.r.pc,
                    1 => s.canonical(),
                    _ => s.canonical().wrapping_add(2),
                };
                s.target(target);
                s.operand = if s.size == 2 { 0x201f } else { 0x201f_0010 };
                let (s, bus) = s.seed();
                let p = plan::validate(s, &bus);
                assert_eq!(p.address, target);
                assert_eq!(
                    model::sample(&bus.words, target),
                    (s.operand >> if s.size == 4 { 16 } else { 0 }) as u16
                );
                for host in [false, true] {
                    let mut b = bus.clone();
                    if host {
                        b.fail = Some(p.final_read);
                    }
                    total.case(s, b);
                }
            }
        }
    }
    total.result()
}
pub(super) fn program_wrap() -> (u32, u32) {
    let mut total = Count::default();
    for form in forms() {
        for supervisor in [false, true] {
            for pc in [0xffff_fffc, 0xffff_fffe] {
                let mut s = bank(form, supervisor, 31);
                s.r.pc = pc;
                let (s, bus) = s.seed();
                let p = plan::validate(s, &bus);
                for path in 0..3 {
                    let mut b = bus.clone();
                    if path == 1 {
                        b.fail = Some(p.final_read);
                    } else if path == 2 {
                        capacity::blocked(&p, &mut b, p.final_read);
                    }
                    total.case(s, b);
                }
            }
        }
    }
    total.result()
}
pub(super) fn operand_wrap() -> (u32, u32) {
    let mut total = Count::default();
    for form in forms() {
        for supervisor in [false, true] {
            for target in [0x00ff_fffe, 0xffff_fffe] {
                if form.mode == 7 && form.reg == 0 && target == 0x00ff_fffe {
                    continue;
                }
                let mut s = bank(form, supervisor, 1);
                s.target(target);
                s.operand = if s.size == 4 { 0x7fff_8000 } else { 0x8000 };
                let (s, bus) = s.seed();
                let p = plan::validate(s, &bus);
                assert_eq!(p.address, target);
                if s.size == 4 {
                    assert_eq!(model::sample(&bus.words, target.wrapping_add(2)), 0x8000);
                }
                for path in 0..3 {
                    let mut b = bus.clone();
                    if path == 1 {
                        b.fail = Some(p.final_read);
                    } else if path == 2 {
                        capacity::blocked(&p, &mut b, p.final_read);
                    }
                    total.case(s, b);
                }
            }
        }
    }
    total.result()
}
pub(super) fn odd() -> (u32, u32) {
    let mut total = Count::default();
    for form in model::encoded() {
        for supervisor in [false, true] {
            for sr in [0x0000, 0x0115, 0x841a, 0x871f] {
                for pair in [3, 4] {
                    let mut s = form;
                    s.r = status(s.r, sr | if supervisor { 0x2000 } else { 0 });
                    (s.operand, s.immediate) = pairs(s.size)[pair];
                    let target = model::address(s, s.extension) | 1;
                    s.target(target);
                    let (s, bus) = s.seed();
                    assert_eq!(plan::validate(s, &bus).address, target);
                    total.case(s, bus);
                }
            }
        }
    }
    total.result()
}
