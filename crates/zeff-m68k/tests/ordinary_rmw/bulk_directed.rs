use super::*;
use bulk_check::Count;
use bulk_model::{Spec, address};

pub(super) fn upper_sr() -> (u32, u32) {
    let mut count = Count::default();
    for family in [0x4200, 0x4600] {
        for size in [2, 4] {
            for reg in [0, 7] {
                for upper in 0..32u16 {
                    let sr = (upper & 16) << 11 | (upper & 8) << 10 | (upper & 7) << 8;
                    for ccr in 0..32 {
                        let mut s = Spec::new(family, size, 3, reg, sr & 0x2000 != 0);
                        s.r.set_status_register(sr | ccr);
                        s.wait = 3;
                        let (s, bus) = s.seed();
                        let plan = bulk_plan::build(s, &bus);
                        for route in 0..4 {
                            let mut b = bus.clone();
                            match route {
                                1 => b.fail = Some(plan.f_index),
                                2 => b.fail = Some(s.callbacks() - 1),
                                3 => bulk_capacity::blocked(&plan, &mut b, plan.f_index),
                                _ => {}
                            }
                            count.planned(s, b, plan.clone());
                        }
                    }
                }
            }
        }
    }
    count.result()
}

pub(super) fn mutations() -> (u32, u32) {
    let mut count = Count::default();
    for base in bulk_model::encoded() {
        let extensions = base.extensions();
        let mut sites = vec![extensions, base.f_index()];
        if base.size == 4 {
            sites.extend([extensions + 1, base.callbacks() - 1]);
        }
        if extensions >= 1 {
            sites.push(0);
        }
        if extensions == 2 {
            sites.push(1);
        }
        for bank in [false, true] {
            let mut s = base.bank(bank, 0x1f);
            s.wait = 3;
            let e = address(s, s.extension);
            let c = s.canonical();
            let p = s.r.pc;
            let (s, original) = s.seed();
            for site in &sites {
                for target in [
                    e,
                    e.wrapping_add(2),
                    c,
                    c.wrapping_add(2),
                    p.wrapping_add(2),
                    p.wrapping_add(4),
                ] {
                    for host in [false, true] {
                        let mut b = original.clone();
                        b.mutation = Some((*site, target, 0x7002));
                        b.fail = host.then_some(*site);
                        if s.extensions() == 2 && *site == 0 && target == p.wrapping_add(4) {
                            let new_ea = u32::from(s.extension[0]) << 16 | 0x7002;
                            s.source_word(&mut b, new_ea, s.operand);
                        }
                        let plan = bulk_plan::build(s, &b);
                        if s.extensions() == 2 && *site == 0 && target == p.wrapping_add(4) {
                            assert_eq!(plan.address, u32::from(s.extension[0]) << 16 | 0x7002);
                        } else {
                            assert_eq!(plan.address, e);
                        }
                        count.planned(s, b, plan);
                    }
                }
            }
        }
    }
    count.result()
}

pub(super) fn retained() -> (u32, u32) {
    let mut count = Count::default();
    for base in bulk_model::forms() {
        for bank in [false, true] {
            for retained in [0x4e71, 0x7001, 0x4200, 0x42c0, 0xffff] {
                let mut s = base.bank(bank, 0x1f);
                s.retained = retained;
                s.wait = 3;
                let (s, bus) = s.seed();
                let plan = bulk_plan::build(s, &bus);
                assert_eq!(plan.retained, retained);
                for target in 0..s.callbacks() {
                    let mut b = bus.clone();
                    b.fail = Some(target);
                    count.planned(s, b, plan.clone());
                }
            }
        }
    }
    count.result()
}
