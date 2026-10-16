use super::*;
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    match index {
        25 => {
            for s in model::encoded() {
                for state in 0..5 {
                    for at in 0..6 {
                        for (k, n) in model::ENTRY_REPORTS {
                            let q = reset::Request {
                                report: Some((at, k, n)),
                                ..Default::default()
                            };
                            tally.add(reset::run(s, state, q, mark));
                        }
                    }
                }
            }
        }
        26 => {
            for s in model::encoded() {
                for state in 0..5 {
                    for sp in [0x90000, 0x90001] {
                        for route in 0usize..6 {
                            let q = reset::Request {
                                sp,
                                level: 6,
                                pc: if route == 0 { 0x7000 } else { 0x7001 },
                                detector: route.saturating_sub(1),
                                ..Default::default()
                            };
                            tally.add(reset::run(s, state, q, mark));
                        }
                    }
                }
            }
        }
        27 => {
            for s in model::encoded() {
                for route in 0..4 {
                    tally.add(reset::rejected(s, route, mark));
                }
            }
        }
        _ => unreachable!(),
    }
    tally.result()
}
