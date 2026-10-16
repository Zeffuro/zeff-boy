use super::*;
pub(super) fn witnesses() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for op in OPS {
        for size in [Size::Word, Size::Long] {
            let long = size == Size::Long;
            let (source, destination, result, partial, ccr) = match op {
                Op::Add => (
                    1,
                    if long { 0x7fff_ffff } else { 0x7fff },
                    if long { 0x8000_0000 } else { 0x8000 },
                    0x7fff_0000,
                    10,
                ),
                Op::Sub => (
                    1,
                    if long { 0x8000_0000 } else { 0x8000 },
                    if long { 0x7fff_ffff } else { 0x7fff },
                    0x8000_ffff,
                    2,
                ),
                Op::And => (
                    if long { 0xffff } else { 0 },
                    if long { 0x8000_0000 } else { 0xffff },
                    0,
                    0x8000_0000,
                    20,
                ),
                Op::Or => (
                    if long { 0x8000_0000 } else { 0x8000 },
                    0,
                    if long { 0x8000_0000 } else { 0x8000 },
                    0,
                    24,
                ),
            };
            for supervisor in [false, true] {
                let mut case = Case::new(Form {
                    op,
                    size,
                    ea: Ea::Post,
                });
                case.r = registers(supervisor, 31);
                case.r.pc = 0x1000;
                case.r.usp = 0x8000;
                case.r.ssp = 0x8000;
                case.register = 7;
                case.value = source;
                case.r.d[5] = if long {
                    destination
                } else {
                    0xaabb_0000 | destination
                };
                let (_, bus) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                let q = plan.last().unwrap();
                let expected_q = if long { partial } else { 0xaabb_0000 | result };
                let control = if supervisor { SUPERVISOR } else { 0 };
                let literal = oracle::value(case, &bus.words) == source
                    && oracle::arithmetic(case, source) == (result, control | ccr)
                    && q.registers.d[5] == expected_q
                    && q.registers.status_register() == control | ccr
                    && q.ird == if long { case.opcode() } else { 0x7201 }
                    && q.internal_pc == 0x1004
                    && q.registers.address_register(7) == Some(if long { 0x8004 } else { 0x8002 });
                for failure in [
                    None,
                    Some((plan.len() - 1, Failure::Host)),
                    Some((plan.len() - 1, Failure::Boundary)),
                ] {
                    count += 1;
                    bad += u32::from(!literal || !run::check(case, failure, 0));
                }
            }
        }
    }
    (count, bad)
}
