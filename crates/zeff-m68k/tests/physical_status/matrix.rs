use super::*;
use device::Kind;

const WORDS: [u16; 8] = [0, 31, 16, 0x2000, 0x8000, 0xa71f, 0x58e0, 0xffff];
fn old(mut s: model::Spec, ccr: u16, interrupt: u16) -> model::Spec {
    s.r = status(
        s.r,
        if s.context == 0 { 0 } else { 0x2000 } | interrupt << 8 | ccr,
    );
    s
}
pub(super) fn completion() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for base in model::encoded() {
        for ccr in 0..32 {
            for interrupt in 0..8 {
                for new_ccr in 0..32 {
                    for upper in [0, 0x2000, 0x8000, 0xa700] {
                        for wait in [0, 3] {
                            let mut s = old(base, ccr, interrupt);
                            s.word = upper | new_ccr;
                            s.wait = wait;
                            count += 1;
                            bad += u32::from(!oracle::ordinary(s, 0));
                        }
                    }
                }
            }
        }
    }
    (count, bad)
}
pub(super) fn faults() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for base in model::encoded() {
        for at in 0..base.count() {
            for ccr in 0..32 {
                for word in WORDS {
                    for interrupt in [0, 1, 4, 7] {
                        for (kind, elapsed) in [
                            (Kind::Host, 7),
                            (Kind::Boundary, 7),
                            (Kind::Unavailable, 7),
                            (Kind::Complete, 3),
                        ] {
                            let mut s = old(base, ccr, interrupt);
                            s.word = word;
                            let (_, mut bus) = s.seed();
                            bus.report = Some((at, kind, elapsed));
                            count += 1;
                            bad += u32::from(!oracle::run(s, bus));
                        }
                    }
                }
            }
        }
    }
    (count, bad)
}
pub(super) fn odd() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for base in model::encoded() {
        for ccr in 0..32 {
            for interrupt in 0..8 {
                for detector in 0..7 {
                    let mut s = old(base, ccr, interrupt);
                    s.odd();
                    let (_, mut bus) = s.seed();
                    bus.default_detector = detector == 0;
                    bus.detector = Some(match detector {
                        1 => (Kind::Complete, 17),
                        2 => (Kind::Host, 17),
                        3 => (Kind::Unavailable, 17),
                        4 => (Kind::Boundary, 0),
                        5 => (Kind::Boundary, 3),
                        _ => (Kind::Boundary, 17),
                    });
                    count += 1;
                    bad += u32::from(!oracle::run(s, bus));
                }
            }
        }
    }
    (count, bad)
}
pub(super) fn mask() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for word in 0..=65535 {
        for context in 0..3 {
            for path in 0..3 {
                let mut s = model::Spec::new(context, 0, 0);
                s.word = word;
                count += 1;
                bad += u32::from(!oracle::ordinary(s, path));
            }
        }
    }
    (count, bad)
}
