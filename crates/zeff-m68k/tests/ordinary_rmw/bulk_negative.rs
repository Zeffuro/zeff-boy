use super::*;
use bulk_check::Count;
use bulk_model::{Spec, address, values};

pub(super) fn bytes() -> (u32, u32) {
    let mut count = Count::default();
    for base in bulk_model::encoded().into_iter().filter(|s| s.size == 2) {
        for bank in [false, true] {
            for parity in 0..2 {
                for ccr in 0..32 {
                    for wait in [0, 3] {
                        let mut s = base.bank(bank, ccr);
                        s.size = 1;
                        s.operand = 0x81;
                        s.wait = wait;
                        s.target(address(s, s.extension) & !1 | parity);
                        let (s, bus) = s.seed();
                        let plan = bulk_plan::build(s, &bus);
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
    count.result()
}

pub(super) fn registers() -> (u32, u32) {
    let mut count = Count::default();
    for family in [0x4200, 0x4600] {
        for size in [1, 2, 4] {
            for reg in 0..8 {
                for bank in [false, true] {
                    for ccr in 0..32 {
                        for operand in values(size) {
                            for wait in [0, 3] {
                                let mut s = Spec::new(family, size, 0, reg, bank).bank(bank, ccr);
                                s.operand = operand;
                                s.wait = wait;
                                let (s, bus) = s.seed();
                                let plan = bulk_plan::build(s, &bus);
                                for host in [false, true] {
                                    let mut b = bus.clone();
                                    b.fail = host.then_some(0);
                                    count.planned(s, b, plan.clone());
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

fn other_encoded() -> Vec<Spec> {
    let mut result = Vec::new();
    for family in [0x4000, 0x4400, 0x4a00] {
        for size in [1, 2, 4] {
            for mode in 2..=7 {
                for reg in 0..if mode == 7 { 2 } else { 8 } {
                    result.push(Spec::new(family, size, mode, reg, false));
                }
            }
        }
    }
    result
}

pub(super) fn other_memory() -> (u32, u32) {
    let mut count = Count::default();
    for base in other_encoded() {
        for bank in [false, true] {
            for ccr in 0..32 {
                for operand in values(base.size) {
                    for wait in [0, 3] {
                        let mut s = base.bank(bank, ccr);
                        s.operand = operand;
                        s.wait = wait;
                        let (s, bus) = s.seed();
                        let plan = bulk_plan::build(s, &bus);
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
    count.result()
}

pub(super) fn other_callbacks() -> (u32, u32) {
    let mut count = Count::default();
    for base in other_encoded() {
        for bank in [false, true] {
            for ccr in [0, 31] {
                for operand in [0, if base.size == 1 { 0x80 } else { 0x8000 }] {
                    let mut s = base.bank(bank, ccr);
                    s.operand = operand;
                    s.wait = 3;
                    let (s, bus) = s.seed();
                    let plan = bulk_plan::build(s, &bus);
                    for target in 0..s.callbacks() {
                        let mut b = bus.clone();
                        b.fail = Some(target);
                        count.planned(s, b, plan.clone());
                    }
                }
            }
        }
    }
    count.result()
}
