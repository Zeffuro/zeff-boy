use super::*;
use device::Kind;
use model::Spec;

pub(super) fn indices() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for base in model::encoded()
        .into_iter()
        .filter(|s| matches!(s.mode, 4 | 8))
    {
        for selector in 0..16u16 {
            for long in [false, true] {
                for disp in [0x10u16, 0xf0] {
                    for negative in [false, true] {
                        for ignored in 0..8 {
                            for path in 0..3 {
                                let mut s = base;
                                s.extension =
                                    selector << 12 | u16::from(long) << 11 | ignored << 8 | disp;
                                let raw = if long {
                                    if negative { 0xffff_fffe } else { 2 }
                                } else if negative {
                                    0x1234_fffe
                                } else {
                                    0x1234_0002
                                };
                                let index = usize::from(selector & 7);
                                if selector < 8 {
                                    s.r.d[index] = raw;
                                } else {
                                    model::set_active(&mut s.r, index, raw);
                                }
                                s.word = [0x0010, 0xa71f, 0x58e0][path as usize];
                                count += 1;
                                bad += u32::from(!operand_path(s, path));
                            }
                        }
                    }
                }
            }
        }
    }
    (count, bad)
}
fn operand_path(s: Spec, path: u8) -> bool {
    if path != 1 {
        return oracle::ordinary(s, path);
    }
    let (_, mut bus) = s.seed();
    bus.report = Some((s.extensions(), Kind::Host, 7));
    oracle::run(s, bus)
}
pub(super) fn displacements() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for mut s in model::encoded()
        .into_iter()
        .filter(|s| matches!(s.mode, 3 | 7))
    {
        for extension in [0, 0x7ffe, 0x8000, 0xfffe] {
            for path in 0..3 {
                s.extension = extension;
                count += 1;
                bad += u32::from(!operand_path(s, path));
            }
        }
    }
    (count, bad)
}
pub(super) fn absolute_word() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for context in 0..3 {
        for extension in [0, 0x7ffe, 0x8000, 0xfffe] {
            for path in 0..3 {
                let mut s = Spec::new(context, 5, 0);
                s.extension = extension;
                count += 1;
                bad += u32::from(!operand_path(s, path));
            }
        }
    }
    (count, bad)
}
pub(super) fn absolute_long() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for context in 0..3 {
        for high in [0, 0x00ff, 0x1234, 0xffff] {
            for low in [0, 0x7ffe, 0x8000, 0xfffe] {
                for path in 0..3 {
                    let mut s = Spec::new(context, 6, 0);
                    s.extension = high;
                    s.low = low;
                    count += 1;
                    bad += u32::from(!operand_path(s, path));
                }
            }
        }
    }
    (count, bad)
}
pub(super) fn sampling() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for s in model::encoded() {
        for at in 0..s.count() {
            for path in 0..3 {
                let (_, mut bus) = s.seed();
                let p = model::plan(s, &bus);
                let t = p.phases[at];
                let value = if at == s.extensions() {
                    0xa710
                } else if s.mode == 6 && at == 0 {
                    s.low ^ 0x0010
                } else if at + 1 == s.count() {
                    0x7202
                } else {
                    0x44fc
                };
                bus.mutation = Some((at, t.access.logical_address, value));
                if s.mode == 6 && at == 0 {
                    bus.word(s.address(value), 0x2004);
                }
                bus.report = match path {
                    0 => None,
                    1 => Some((at, Kind::Host, 7)),
                    _ => Some((at, Kind::Boundary, 7)),
                };
                count += 1;
                bad += u32::from(!oracle::run(s, bus));
            }
        }
    }
    (count, bad)
}
pub(super) fn overlap() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for base in model::forms() {
        for target in 0..3 {
            for path in 0..3 {
                let mut s = base;
                let address = match target {
                    0 => s.r.pc,
                    1 => s.canonical(),
                    _ => s.canonical() + 2,
                };
                if s.mode == 7 {
                    s.extension = [0xfffe, 2, 4][target];
                } else if s.mode == 8 {
                    s.extension = [0x30fc, 0x3000, 0x3002][target];
                } else {
                    s.target(address);
                }
                s.word = 0x201f;
                let (_, mut bus) = s.seed();
                if target == 1 {
                    bus.mutation = Some((s.count() - 2, s.canonical(), 0x4e71));
                }
                bus.report = match path {
                    0 => None,
                    1 => Some((s.count() - 2, Kind::Host, 7)),
                    _ => Some((s.count() - 1, Kind::Boundary, 7)),
                };
                count += 1;
                bad += u32::from(!oracle::run(s, bus));
            }
        }
    }
    (count, bad)
}
pub(super) fn mutations() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for s in model::forms() {
        for scenario in 0..4 {
            for path in 0..3 {
                let (_, mut bus) = s.seed();
                let first = s.count() - 2;
                bus.mutation = Some(match scenario {
                    0 => (first, s.canonical(), 0x44fc),
                    1 => (first + 1, s.canonical().wrapping_add(2), 0x46fc),
                    2 => (s.extensions(), s.r.pc.wrapping_add(2), 0x44d0),
                    _ => (first, s.canonical() | 0x0100_0000, 0x46d0),
                });
                bus.report = match path {
                    0 => None,
                    1 => Some((first, Kind::Host, 7)),
                    _ => Some((first + 1, Kind::Boundary, 7)),
                };
                count += 1;
                bad += u32::from(!oracle::run(s, bus));
            }
        }
    }
    (count, bad)
}
pub(super) fn program_wrap() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for mut s in model::forms() {
        for pc in [0xffff_fffc, 0xffff_fffe] {
            for path in 0..3 {
                s.r.pc = pc;
                count += 1;
                bad += u32::from(!oracle::ordinary(s, path));
            }
        }
    }
    (count, bad)
}
pub(super) fn operand_wrap() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for base in model::forms() {
        for address in [0x00ff_fffe, 0xffff_fffe] {
            if base.mode == 5 && address == 0x00ff_fffe {
                continue;
            }
            for path in 0..3 {
                let mut s = base;
                s.target(address);
                count += 1;
                bad += u32::from(!oracle::ordinary(s, path));
            }
        }
    }
    (count, bad)
}
