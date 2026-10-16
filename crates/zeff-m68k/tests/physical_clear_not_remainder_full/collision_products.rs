use super::device::Device;
use super::model::Failure;
use super::{check, matrix, seed};

pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    assert_eq!(index, 37);
    let (mut count, mut bad) = (0, 0);
    seed::each(|base| {
        for at in 0..base.p.count() {
            for sp in [0, 2, 14, 0xffff_fff0, 0x00ff_fff0, 0x8000_0010] {
                for handler in [
                    0x4000,
                    0xffff_fffc,
                    0xffff_fffe,
                    0x00ff_fffc,
                    0x00ff_fffe,
                    0,
                ] {
                    for value in [0, 1u32 << (base.p.bits - 1)] {
                        for wait in [4, 7] {
                            let mut c = base;
                            c.r.ssp = sp;
                            c.handler = handler;
                            c = matrix::source(c, value);
                            let good = check::run(
                                c,
                                |b: &mut Device| {
                                    for report in b.primary.iter_mut().take(c.p.count()) {
                                        report.elapsed = wait;
                                    }
                                    for report in &mut b.entry {
                                        report.elapsed = wait;
                                    }
                                    b.fault(at, Failure::Boundary, 8);
                                },
                                mark,
                            );
                            count += 1;
                            bad += u32::from(!good);
                        }
                    }
                }
            }
        }
    });
    assert_eq!(count, 180864);
    (count, bad)
}
