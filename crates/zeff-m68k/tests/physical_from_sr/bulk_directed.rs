use super::*;
use bulk_device::Kind;
use bulk_matrix::{Count, bank};
use bulk_model::{Spec, encoded, forms, set_active, status};

pub(super) fn indexes() -> (u32, u32) {
    let mut total = Count::default();
    for family in [0x40c0] {
        for size in [2] {
            for base in 0..8 {
                for supervisor in [false, true] {
                    for selector in 0..16 {
                        for long in [false, true] {
                            for ignored in 0..8 {
                                for displacement in [-128i8, -1, 0, 127] {
                                    for value in if long {
                                        &[0u32, 0x7fff_ffff, 0x8000_0000, 0xffff_ffff][..]
                                    } else {
                                        &[0u32, 0x7fff, 0x8000, 0xffff][..]
                                    } {
                                        for wait in [4, 7] {
                                            let mut s =
                                                Spec::new(family, size, 4, base, supervisor);
                                            s.r.usp = 0x9000;
                                            s.r.ssp = 0x9000;
                                            set_active(&mut s.r, base, 0x1000);
                                            if selector < 8 {
                                                s.r.d[selector] = *value;
                                            } else {
                                                set_active(&mut s.r, selector - 8, *value);
                                            }
                                            s.extension = ((selector as u16) << 12)
                                                | (u16::from(long) * 0x0800)
                                                | ((ignored as u16) << 8)
                                                | u16::from(displacement as u8);
                                            s.wait = wait;
                                            s.operand =
                                                if size == 2 { 0x8001 } else { 0x8000_0001 };
                                            s.r = status(
                                                s.r,
                                                if supervisor { 0x2014 } else { 0x0014 },
                                            );
                                            let address = s.address(s.low);
                                            let (_, mut b) = s.seed();
                                            let p = bulk_matrix::validate(s, &b);
                                            assert_eq!(p.address, address);
                                            b.default_entry_detector = true;
                                            total.case(s, b);
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
    total.result()
}
pub(super) fn wraps() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded() {
        for supervisor in [false, true] {
            for pc in [
                0x2000,
                0xffff_fffc,
                0xffff_fffe,
                0x00ff_fffc,
                0x00ff_fffe,
                0,
            ] {
                for address in if base.mode == 7 {
                    &[0x7000u32][..]
                } else if base.mode == 5 {
                    &[0u32, 2, 0x7ffe, 0xffff_8000, 0xffff_fffc, 0xffff_fffe][..]
                } else {
                    &[0u32, 2, 0x7fff_fffe, 0xffff_fffe, 0x00ff_fffe, 0xffff_fffc][..]
                } {
                    for wait in [4, 7] {
                        let mut s = bank(base, supervisor, 31);
                        s.r.pc = pc;
                        s.target(*address);
                        s.wait = wait;
                        let (_, mut b) = s.seed();
                        if s.mode == 6 {
                            b.word(s.r.pc.wrapping_add(4), s.low);
                        }
                        let p = bulk_plan::plan(s, &b);
                        assert_eq!(p.address, *address);
                        total.case(s, b);
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn upper() -> (u32, u32) {
    let mut total = Count::default();
    for form in [0, 7] {
        for reg in [0, 7] {
            for trace in [0, 0x8000] {
                for supervisor in [0, 0x2000] {
                    for mask in 0..8 {
                        for ccr in 0..32 {
                            for path in 0..3 {
                                let mut s = Spec::new(0x40c0, 2, form, reg, supervisor != 0);
                                s.r = status(s.r, trace | supervisor | (mask << 8) | ccr);
                                let (mut cpu, mut b) = s.seed();
                                if path != 0 {
                                    let at = if form == 7 {
                                        0
                                    } else if path == 1 {
                                        s.count() - 2
                                    } else {
                                        s.count() - 1
                                    };
                                    b.report = Some((
                                        at,
                                        if path == 1 {
                                            Kind::Host
                                        } else {
                                            Kind::Boundary
                                        },
                                        if path == 1 { 6 } else { 8 },
                                    ));
                                }
                                if trace != 0 {
                                    let saved = cpu.clone();
                                    let device = b.clone();
                                    total.record(
                                        cpu.step_physical_instruction(&mut b)
                                            == Err(InstructionFault::TraceUnqualified)
                                            && cpu == saved
                                            && b == device,
                                    );
                                } else {
                                    total.case(s, b);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    total.result()
}

pub(super) fn mutations() -> (u32, u32) {
    let mut total = Count::default();
    let mut geometry = 0;
    for base in encoded() {
        let (_, b) = base.seed();
        let p = bulk_plan::plan(base, &b);
        let sites: Vec<_> = p.phases.iter().enumerate().map(|(i, _)| i).collect();
        geometry += sites.len();
        for supervisor in [false, true] {
            let s = bank(base, supervisor, 31);
            let (_, b) = s.seed();
            let p = bulk_plan::plan(s, &b);
            for at in &sites {
                for target in [
                    p.address,
                    p.address.wrapping_add(2),
                    s.canonical(),
                    s.canonical().wrapping_add(2),
                    s.r.pc.wrapping_add(2),
                    s.r.pc.wrapping_add(4),
                ] {
                    for (kind, elapsed) in
                        [(Kind::Complete, 4), (Kind::Host, 6), (Kind::Boundary, 8)]
                    {
                        let mut bus = b.clone();
                        bus.mutation = Some((*at, target, 0x7002));
                        let changed = bulk_plan::plan(s, &bus);
                        if changed.address != p.address {
                            s.source(&mut bus, changed.address, s.operand);
                        }
                        assert_eq!(bulk_plan::plan(s, &bus).address, changed.address);
                        bus.report = Some((*at, kind, elapsed));
                        total.case(s, bus);
                    }
                }
            }
        }
    }
    assert_eq!(geometry, 153);
    total.result()
}
pub(super) fn overlaps() -> (u32, u32) {
    let mut total = Count::default();
    for base in forms() {
        for supervisor in [false, true] {
            for layout in 0..8 {
                for path in 0..3 {
                    let mut s = bank(base, supervisor, 31);
                    s.r.ssp = 0x7000;
                    let address = match layout {
                        0 => s.r.pc,
                        1 => s.r.pc.wrapping_add(2),
                        2 => s.canonical(),
                        3 => s.canonical().wrapping_add(2),
                        4 => s.canonical().wrapping_sub(2),
                        5 => s.r.ssp - 14,
                        6 => s.r.ssp - 8,
                        _ => s.r.ssp - 2,
                    };
                    s.target(address);
                    let (_, mut b) = s.seed();
                    if s.mode == 6 {
                        b.word(s.r.pc.wrapping_add(4), s.low);
                    }
                    let p = bulk_plan::plan(s, &b);
                    assert_eq!(p.address, address);
                    if path != 0 {
                        let at = if path == 1 {
                            p.phases
                                .iter()
                                .position(|p| p.phase == InstructionPhase::OperandRefill)
                                .unwrap()
                        } else {
                            s.count() - 1
                        };
                        b.report = Some((at, Kind::Boundary, 8));
                    }
                    total.case(s, b);
                }
            }
        }
    }
    total.result()
}
pub(super) fn stack_overlays() -> (u32, u32) {
    let mut total = Count::default();
    for family in [0x40c0] {
        for size in [2] {
            for mode in 0..3 {
                for supervisor in [false, true] {
                    for path in 0..3 {
                        let mut s = Spec::new(family, size, mode, 7, supervisor);
                        s.target(0x1000);
                        if !supervisor {
                            s.r.ssp = 0x1000 + u32::from(size);
                        }
                        let (_, mut b) = s.seed();
                        bulk_matrix::validate(s, &b);
                        if path != 0 {
                            b.report = Some((
                                if path == 1 {
                                    if size == 2 { 1 } else { 2 }
                                } else {
                                    s.count() - 1
                                },
                                Kind::Boundary,
                                8,
                            ));
                        }
                        total.case(s, b);
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn retained() -> (u32, u32) {
    let mut total = Count::default();
    let mut geometry = 0;
    for base in forms() {
        geometry += base.count();
        for supervisor in [false, true] {
            for first in [0x4e71, 0x7001, 0x4000, 0x40c0, 0xffff] {
                let mut s = bank(base, supervisor, 31);
                s.first = first;
                let (_, b) = s.seed();
                bulk_matrix::validate(s, &b);
                for at in 0..s.count() {
                    let mut bus = b.clone();
                    bus.report = Some((at, Kind::Boundary, 8));
                    total.case(s, bus);
                }
            }
        }
    }
    assert_eq!(geometry, 27);
    total.result()
}
