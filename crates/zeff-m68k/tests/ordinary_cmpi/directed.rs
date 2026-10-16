use super::*;
use model::{bank, encoded, pairs, status};
use oracle::Count;
use plan::Action;

pub(super) fn mutation() -> (u32, u32) {
    let mut total = Count::default();
    for form in encoded() {
        for supervisor in [false, true] {
            for pair in [0, 3, 4, 9] {
                let mut s = bank(form, supervisor, 16);
                (s.operand, s.immediate) = pairs(s.size)[pair];
                let (s, bus) = s.seed();
                let p = plan::validate(s, &bus);
                let reads: Vec<_> = p
                    .actions
                    .iter()
                    .filter_map(|a| {
                        if let Action::Read(access, value) = a {
                            Some((*access, *value))
                        } else {
                            None
                        }
                    })
                    .collect();
                for (at, (access, value)) in reads.into_iter().enumerate() {
                    let mut changed = bus.clone();
                    changed.mutation = Some((at, access.logical_address, value ^ 2));
                    let preview = plan::build(s, &changed);
                    if preview.address != p.address {
                        s.source_word(&mut changed, preview.address, s.operand);
                    }
                    let altered = plan::build(s, &changed);
                    assert_eq!(altered.address, preview.address);
                    if preview.address != p.address {
                        assert_eq!(altered.operand, s.operand & model::mask(s.size));
                    }
                    assert_eq!(
                        altered.immediate
                            & if s.size == 4 {
                                0xffff_0000
                            } else {
                                model::mask(s.size)
                            },
                        s.immediate
                            & if s.size == 4 {
                                0xffff_0000
                            } else {
                                model::mask(s.size)
                            }
                    );
                    for host in [false, true] {
                        let mut b = changed.clone();
                        if host {
                            b.fail = Some(at);
                        }
                        total.case(s, b);
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn whole_sr() -> (u32, u32) {
    let mut total = Count::default();
    for form in encoded() {
        for supervisor in [false, true] {
            for sr in [0x0000, 0x0115, 0x841a, 0x871f] {
                for pair in [0, 3, 4, 9] {
                    let mut s = form;
                    s.r = status(s.r, sr | if supervisor { 0x2000 } else { 0 });
                    (s.operand, s.immediate) = pairs(s.size)[pair];
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
    }
    total.result()
}
