use super::*;
use bulk_check::Count;
use bulk_model::{Spec, active, address, set_active, sized_value};

pub(super) fn indexes() -> (u32, u32) {
    let mut count = Count::default();
    for family in [0x4200, 0x4600] {
        for size in [2, 4] {
            for base in 0..8 {
                for bank in [false, true] {
                    for selector in 0..16 {
                        for long in [false, true] {
                            for ignored in 0..8 {
                                for displacement in [-128i8, -1, 0, 127] {
                                    let inputs = if long {
                                        [0, 0x7fffffff, 0x80000000, 0xffffffff]
                                    } else {
                                        [0, 0x7fff, 0x8000, 0xffff]
                                    };
                                    for input in inputs {
                                        for wait in [0, 3] {
                                            let mut s = Spec::new(family, size, 6, base, bank);
                                            s.r.usp = 0x9000;
                                            s.r.ssp = 0x9000;
                                            set_active(&mut s.r, base, 0x1000);
                                            if selector < 8 {
                                                s.r.d[selector] = input;
                                            } else {
                                                set_active(&mut s.r, selector - 8, input);
                                            }
                                            s.extension[0] = (selector as u16) << 12
                                                | u16::from(long) << 11
                                                | ignored << 8
                                                | u16::from(displacement as u8);
                                            s.wait = wait;
                                            s.operand =
                                                if family == 0x4200 { 0x80000001 } else { 0x8000 };
                                            let base_value =
                                                if selector == base + 8 { input } else { 0x1000 };
                                            let index_value = if long {
                                                input
                                            } else {
                                                i32::from(input as i16) as u32
                                            };
                                            let expected_ea = base_value
                                                .wrapping_add(index_value)
                                                .wrapping_add_signed(i32::from(displacement));
                                            assert_eq!(active(s.r, base), base_value);
                                            let (s, bus) = s.seed();
                                            let plan = bulk_plan::build(s, &bus);
                                            assert_eq!(plan.address, expected_ea);
                                            assert_eq!(
                                                plan.operand,
                                                sized_value(&bus.words, expected_ea, size)
                                            );
                                            for host in [false, true] {
                                                let mut b = bus.clone();
                                                b.fail = host.then_some(plan.f_index);
                                                count.planned(s, b, plan.clone());
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
    }
    count.result()
}

pub(super) fn wraps() -> (u32, u32) {
    let mut count = Count::default();
    for base in bulk_model::encoded() {
        for bank in [false, true] {
            for pc in [0x2000, 0xfffffffc, 0xfffffffe, 0x00fffffc, 0x00fffffe, 0] {
                let targets = if base.mode == 7 && base.reg == 0 {
                    [0, 2, 0x7ffe, 0xffff8000, 0xfffffffc, 0xfffffffe]
                } else {
                    [0, 2, 0x7ffffffe, 0xfffffffe, 0x00fffffe, 0xfffffffc]
                };
                for target in targets {
                    for wait in [0, 3] {
                        let mut s = base.bank(bank, 0x1f);
                        s.r.pc = pc;
                        s.target(target);
                        s.wait = wait;
                        let (s, bus) = s.seed();
                        let plan = bulk_plan::build(s, &bus);
                        assert_eq!(plan.address, target);
                        assert_eq!(plan.operand, sized_value(&bus.words, target, s.size));
                        count.planned(s, bus, plan);
                    }
                }
            }
        }
    }
    count.result()
}

pub(super) fn overlaps() -> (u32, u32) {
    let mut count = Count::default();
    for base in bulk_model::forms() {
        for bank in [false, true] {
            let s = base.bank(bank, 0x1f);
            let p = s.r.pc;
            let c = s.canonical();
            for target in [p, p + 2, c, c + 2, c - 2, c + 4, p - 2, p + 6] {
                let mut s = s;
                s.target(target);
                s.wait = 3;
                let (s, bus) = s.seed();
                let plan = bulk_plan::build(s, &bus);
                assert_eq!(plan.address, target);
                assert_eq!(plan.operand, sized_value(&bus.words, target, s.size));
                assert_eq!(address(s, s.extension), target);
                for route in 0..3 {
                    let mut b = bus.clone();
                    b.fail = match route {
                        0 => None,
                        1 => Some(plan.f_index),
                        _ => Some(s.callbacks() - 1),
                    };
                    count.planned(s, b, plan.clone());
                }
            }
        }
    }
    count.result()
}
