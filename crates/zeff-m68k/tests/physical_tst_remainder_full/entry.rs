use super::*;
use device::Kind;
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    match index {
        23 => {
            for (s, primary) in model::triggers() {
                for at in 0..11 {
                    for (k, n) in model::ENTRY_REPORTS {
                        tally.add(check::run(
                            s,
                            |b| {
                                b.report = Some((primary, Kind::Boundary, 8));
                                b.entry_failure = Some((at, k, n));
                            },
                            mark,
                        ));
                    }
                }
                for at in 0..7 {
                    for (k, n) in model::REPORTS {
                        tally.add(check::run(
                            s,
                            |b| {
                                b.report = Some((primary, Kind::Boundary, 8));
                                b.entry_failure = Some((at, k, n));
                                b.entry_write_deposit = true;
                            },
                            mark,
                        ));
                    }
                }
            }
        }
        24 => {
            for (s, primary) in model::triggers() {
                for site in 0..2 {
                    for address in if site == 0 {
                        [0x9001, 0xffff_fffd]
                    } else {
                        [0x4001, 0x00ff_ffff]
                    } {
                        for detector in 0..5 {
                            for source in 0..2 {
                                let mut row = s;
                                row.value = if source == 0 { 0 } else { 1 << (s.bits() - 1) };
                                row.lane = 0;
                                row.preserve_sp = true;
                                if site == 0 {
                                    row.ssp = address;
                                } else {
                                    row.intended = Some(address);
                                }
                                tally.add(check::run(
                                    row,
                                    |b| {
                                        b.report = Some((primary, Kind::Boundary, 8));
                                        b.default_entry_detector = detector == 0;
                                        b.entry_detector = if detector == 0 {
                                            None
                                        } else {
                                            Some(model::DETECTORS[detector])
                                        };
                                    },
                                    mark,
                                ));
                            }
                        }
                    }
                }
            }
        }
        30 => {
            let (mut redirected, mut odd, mut completed) = (0, 0, 0);
            for (s, primary) in model::triggers() {
                for sp in [0, 2, 14, 0xffff_fff0, 0x00ff_fff0, 0x8000_0010] {
                    for handler in [
                        0x4000,
                        0xffff_fffc,
                        0xffff_fffe,
                        0x00ff_fffc,
                        0x00ff_fffe,
                        0,
                    ] {
                        for source in 0..2 {
                            for wait in [4, 7] {
                                let mut row = s;
                                row.ssp = sp;
                                row.preserve_sp = true;
                                row.intended = Some(handler);
                                row.value = if source == 0 { 0 } else { 1 << (s.bits() - 1) };
                                row.desired = 0x1000 | source;
                                row.lane = source;
                                row.wait = wait;
                                let (_, _, mut b, _, _) = row.seed();
                                b.report = Some((primary, Kind::Boundary, 8));
                                let e = oracle::predict(row, &b);
                                let vector: Vec<_> = e
                                    .events
                                    .iter()
                                    .filter_map(|event| {
                                        if let device::Event::Transfer(
                                            device::Stage::Entry,
                                            a,
                                            _,
                                            v,
                                            _,
                                            _,
                                            _,
                                            _,
                                        ) = event
                                            && a.kind == AccessKind::Vector
                                        {
                                            Some(*v)
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();
                                let actual_handler =
                                    u32::from(vector[0]) << 16 | u32::from(vector[1]);
                                redirected += u32::from(actual_handler != handler);
                                odd += u32::from(actual_handler & 1 != 0);
                                completed += u32::from(e.guard.is_none());
                                tally.add(check::run(
                                    row,
                                    |b| b.report = Some((primary, Kind::Boundary, 8)),
                                    mark,
                                ));
                            }
                        }
                    }
                }
            }
            tally.bad += u32::from((redirected, odd, completed) != (12192, 2952, 33624));
        }
        _ => unreachable!(),
    }
    tally.result()
}
