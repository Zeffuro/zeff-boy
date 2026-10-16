use super::*;
pub(super) fn relocate(mut c: Case, target: u32) -> Case {
    match c.form.ea {
        Ea::Indirect | Ea::Post | Ea::Pre | Ea::Displacement | Ea::Index => {
            let base = match c.form.ea {
                Ea::Pre => target.wrapping_add(super::super::oracle::step(c)),
                Ea::Displacement | Ea::Index => target.wrapping_add(16),
                _ => target,
            };
            c.r.set_address_register(usize::from(c.register), base);
        }
        Ea::AbsoluteWord => c.extension = target as u16,
        Ea::AbsoluteLong => {
            c.extension = (target >> 16) as u16;
            c.low = target as u16;
        }
        Ea::Dn => {}
    }
    c
}
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut t = Tally::default();
    model::each(|base| match index {
        6 if !base.form.tail() => {
            for next in [0x4afc, 0xffff, 0x4e70, 0x0c00] {
                for ccr in [0, 31] {
                    for wait in [0, 3] {
                        let mut c = model::status(base, ccr);
                        c.next = next;
                        t.add(check::run(
                            c,
                            |b| {
                                b.elapsed = [4 + wait; 7];
                                b.failure = Some((c.form.count() - 1, Failure::Boundary));
                            },
                            mark,
                        ));
                    }
                }
            }
        }
        7 => {
            let config = Device::seed(base);
            let p = super::super::oracle::plan(base, &config.words.memory());
            for transfer in p.iter().filter(|p| {
                matches!(
                    p.phase,
                    InstructionPhase::OperandRefill | InstructionPhase::OperandReadHigh
                )
            }) {
                for failure in [None, Some(Failure::Host), Some(Failure::Boundary)] {
                    t.add(check::run(
                        base,
                        |b| {
                            b.mutation = Some((transfer.access.logical_address, 0x0080));
                            b.failure = failure.map(|kind| (base.form.count() - 1, kind));
                        },
                        mark,
                    ));
                }
            }
        }
        10 if base.form.ea == Ea::Index => {
            for selector in 0..16u16 {
                for long in [false, true] {
                    for ignored in 0..8u16 {
                        for raw in [0, 0x7fff, 0xffff_8000, 0x1234_0001] {
                            for displacement in [0x80, 0xff, 0, 0x7f] {
                                let mut c = base;
                                c.extension = selector << 12
                                    | u16::from(long) << 11
                                    | ignored << 8
                                    | displacement;
                                if selector < 8 {
                                    c.r.d[usize::from(selector)] = raw;
                                } else {
                                    c.r.set_address_register(usize::from(selector - 8), raw);
                                }
                                t.add(check::run(c, |_| {}, mark));
                            }
                        }
                    }
                }
            }
        }
        11 => {
            for pc in [0x00ff_fffc, 0xffff_fffc, 0xabff_fffc] {
                let mut c = base;
                c.r.pc = pc;
                t.add(check::run(c, |_| {}, mark));
            }
        }
        12 if base.form.ea != Ea::Dn => {
            for target in [
                0x00ff_ffff,
                0x0100_0000,
                u32::MAX,
                0,
                0x00ff_fffe,
                0x8000_0001,
            ] {
                t.add(check::run(relocate(base, target), |_| {}, mark));
            }
        }
        13 if base.form.ea != Ea::Dn => {
            for offset in [0, 2, base.form.length(), base.form.length() + 2] {
                let c = relocate(base, base.r.pc.wrapping_add(offset));
                for fault in [false, true] {
                    t.add(check::run(
                        c,
                        |b| b.failure = fault.then_some((c.form.count() - 1, Failure::Boundary)),
                        mark,
                    ));
                }
            }
        }
        20 => {
            for at in 0..base.form.count() {
                for sp in [
                    8u32,
                    10,
                    12,
                    14,
                    16,
                    18,
                    20,
                    22,
                    0x4321_4002,
                    0x4321_4008,
                    0,
                    0x0100_0008,
                ] {
                    let mut c = base;
                    c.r.ssp = sp | (base.r.ssp & 1);
                    t.add(check::run(
                        c,
                        |b| b.failure = Some((at, Failure::Boundary)),
                        mark,
                    ));
                }
            }
        }
        21 if base.form.tail() => {
            for next in [0x4afc, 0xffff, 0x4e70] {
                let mut c = base;
                c.next = next;
                t.add(check::run(
                    c,
                    |b| b.failure = Some((2, Failure::Boundary)),
                    mark,
                ));
            }
        }
        27 => {
            let c = model::pair(base, (0, 0), 0xa500);
            t.add(check::run(
                c,
                |b| b.word(c.r.pc.wrapping_add(2), 0xffff),
                mark,
            ));
        }
        28 if base.form.ea == Ea::Displacement => {
            for displacement in [0, 1, 0x7fff, 0x8000, 0xffff, 0x00ff] {
                let mut c = base;
                c.extension = displacement;
                t.add(check::run(c, |_| {}, mark));
            }
        }
        29 => {
            for route in 0..4 {
                t.add(preflight(base, route, mark));
            }
        }
        _ => {}
    });
    t.finish()
}
fn preflight(c: Case, route: u8, mark: fn(bool)) -> bool {
    let config = Device::seed(c);
    mark(true);
    let mut b = config.clone();
    let mut cpu = if route == 3 {
        let mut cpu = PrefetchCpu::default();
        cpu.registers = c.r;
        cpu
    } else {
        PrefetchCpu::from_prefetched(c.r, [c.opcode(), c.retained()], CpuClocks(c.start)).unwrap()
    };
    let expected = match route {
        0 => {
            cpu.registers
                .set_status_register(cpu.registers.status_register() | TRACE);
            InstructionFault::TraceUnqualified
        }
        1 => {
            cpu.interrupt_input().set_level(7);
            cpu.interrupt_input().set_level(0);
            InstructionFault::InterruptUnqualified
        }
        _ => {
            if route == 2 {
                cpu.registers.pc = cpu.registers.pc.wrapping_add(2);
            }
            InstructionFault::Execution(TimingFault::Unprimed)
        }
    };
    let saved = cpu.clone();
    let device = b.clone();
    let mut copy = saved.clone();
    let mut copied = device.clone();
    let mut good = check::step(&mut copy, &mut copied) == Err(expected.clone());
    good &= copy == saved && copied == device;
    good &= check::step(&mut cpu, &mut b) == Err(expected);
    good &= cpu == saved && b == device;
    mark(false);
    good
}
