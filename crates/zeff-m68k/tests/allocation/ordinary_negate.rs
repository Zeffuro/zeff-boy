use super::*;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, Direction, FunctionCode};
use zeff_m68k::timing::TimingFault;

#[path = "ordinary_negate/check.rs"]
mod check;
#[path = "ordinary_negate/device.rs"]
mod device;
#[path = "ordinary_negate/plan.rs"]
mod plan;

const WORDS: usize = 0x4000;
const MEMORY_MASK: usize = 0x7fff;
const EVENTS: usize = 12;
const BUDGET: u64 = 4 + u32::MAX as u64;
const VALUES: [[u32; 8]; 2] = [
    [0, 1, 0x7fff, 0x8000, 0xffff, 0x8001, 0xfffe, 0x5555],
    [
        0,
        1,
        0x7fff_ffff,
        0x8000_0000,
        0xffff_ffff,
        0x8000,
        0xffff_0000,
        0x1234_5678,
    ],
];

#[derive(Clone, Copy)]
enum Path {
    Complete,
    Host(usize),
    Reserve(usize),
    Odd,
}

pub(super) fn negate_steps() {
    let mut device = device::Device::new();
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut cases = 0;
    for family in [0x4000, 0x4400] {
        for long in [false, true] {
            for mode in 2..=7 {
                for reg in 0..if mode == 7 { 2 } else { 8 } {
                    for bank in [0, 0x2000] {
                        for ccr in 0..32 {
                            for value in VALUES[usize::from(long)] {
                                for _ in 0..5 {
                                    let spec = plan::Spec::new(
                                        family,
                                        long,
                                        mode,
                                        reg,
                                        bank | 0x500 | ccr,
                                        value,
                                    );
                                    check::run(&mut device, spec, Path::Complete);
                                    cases += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 430_080);
    for _ in 0..100 {
        for family in [0x4000, 0x4400] {
            for long in [false, true] {
                for mode in 2..=7 {
                    for reg in 0..if mode == 7 { 2 } else { 8 } {
                        for status in [0x051f, 0x251f] {
                            let spec = plan::Spec::new(family, long, mode, reg, status, 0x8000);
                            check::run(&mut device, spec, Path::Complete);
                            cases += 1;
                            for at in 0..spec.callbacks() {
                                for path in [Path::Host(at), Path::Reserve(at)] {
                                    check::run(&mut device, spec, path);
                                    cases += 1;
                                }
                            }
                            if reg == 0 || mode == 7 {
                                check::run(&mut device, spec, Path::Odd);
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 768_480);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
