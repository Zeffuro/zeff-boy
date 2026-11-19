use super::*;
use model::{bank, encoded, pairs};
use oracle::Count;

pub(super) fn main_paths(path: u8) -> (u32, u32) {
    let mut total = Count::default();
    let mut geometry = 0;
    for size in [2, 4] {
        let s = model::Spec::new(size, 2, 0, false);
        for ((operand, immediate), flags) in pairs(size)
            .into_iter()
            .zip([4, 9, 0, 11, 2, 4, 8, 0, 11, 0])
        {
            assert_eq!(
                arithmetic::evaluate(s, operand, immediate).1,
                0x0510 | flags
            );
        }
    }
    for form in encoded() {
        let (s, b) = form.seed();
        geometry += capacity::callbacks(&plan::validate(s, &b));
        for supervisor in [false, true] {
            for ccr in 0..32 {
                for (operand, immediate) in pairs(form.size) {
                    let mut s = bank(form, supervisor, ccr);
                    s.operand = operand;
                    s.immediate = immediate;
                    for wait in if path == 0 { &[0, 3][..] } else { &[3][..] } {
                        s.wait = *wait;
                        let (s, bus) = s.seed();
                        let p = plan::validate(s, &bus);
                        if path == 0 {
                            total.case(s, bus);
                        } else {
                            for at in 0..capacity::callbacks(&p) {
                                let mut b = bus.clone();
                                if path == 1 {
                                    b.fail = Some(at);
                                } else {
                                    capacity::blocked(&p, &mut b, at);
                                }
                                total.case(s, b);
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(geometry, 374);
    total.result()
}
pub(super) fn max_wait() -> (u32, u32) {
    let mut total = Count::default();
    for form in encoded() {
        for supervisor in [false, true] {
            for ccr in [0, 1, 16, 31] {
                for pair in [0, 3, 4, 9] {
                    let mut s = bank(form, supervisor, ccr);
                    (s.operand, s.immediate) = pairs(s.size)[pair];
                    let (s, b) = s.seed();
                    let p = plan::validate(s, &b);
                    for at in 0..capacity::callbacks(&p) {
                        for host in [false, true] {
                            let mut bus = b.clone();
                            bus.waits[at] = u32::MAX;
                            if host {
                                bus.fail = Some(at);
                            }
                            total.case(s, bus);
                        }
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn exact_final() -> (u32, u32) {
    let mut total = Count::default();
    for form in encoded() {
        for supervisor in [false, true] {
            for ccr in [0, 1, 16, 31] {
                for pair in [0, 3, 4, 9] {
                    let mut s = bank(form, supervisor, ccr);
                    (s.operand, s.immediate) = pairs(s.size)[pair];
                    let (s, mut b) = s.seed();
                    let p = plan::validate(s, &b);
                    capacity::exact(&p, &mut b, p.final_read);
                    total.case(s, b);
                }
            }
        }
    }
    total.result()
}
pub(super) fn source_idle() -> (u32, u32) {
    let mut total = Count::default();
    for size in [2, 4] {
        for supervisor in [false, true] {
            for mode in [4, 6] {
                for reg in 0..8 {
                    for room in [1, 2] {
                        let (s, mut bus) = model::Spec::new(size, mode, reg, supervisor).seed();
                        let p = plan::validate(s, &bus);
                        capacity::source_idle(&p, &mut bus, s.immediate_words() - 1, room);
                        total.case(s, bus);
                    }
                }
            }
        }
    }
    total.result()
}
