use super::*;

pub(super) fn case(word: u16, bank: bool, lane: u32) -> Case {
    let mode = word >> 3 & 7;
    let size = match word >> 6 & 3 {
        0 => Size::Byte,
        1 => Size::Word,
        _ => Size::Long,
    };
    let mut c = Case::new(Form {
        size,
        ea: match mode {
            0 => Ea::Dn,
            2 => Ea::Indirect,
            3 => Ea::Post,
            4 => Ea::Pre,
            5 => Ea::Displacement,
            6 => Ea::Index,
            _ if word & 7 == 0 => Ea::AbsoluteWord,
            _ => Ea::AbsoluteLong,
        },
    });
    c.r = registers(bank, 0x14);
    c.r.pc = 0x1000;
    c.register = word & 7;
    c.value = match size {
        Size::Byte => 0x7f,
        Size::Word => 0x7fff,
        Size::Long => 0x7fff_ffff,
    };
    c.immediate = match size {
        Size::Byte => 0xff80,
        Size::Word => 0x8000,
        Size::Long => 0x8000_0000,
    };
    c.next = if c.form.tail() { 0x4afc } else { 0x4e71 };
    c.last = 0x7201;
    c.start = 37;
    let ea = 0x3000 | lane;
    match c.form.ea {
        Ea::Dn => {
            c.r.d[usize::from(c.register)] = c.value
                | match size {
                    Size::Byte => 0xaabb_cc00,
                    Size::Word => 0xaabb_0000,
                    Size::Long => 0,
                };
        }
        Ea::Indirect | Ea::Post | Ea::Pre | Ea::Displacement | Ea::Index => {
            let base = match c.form.ea {
                Ea::Pre => ea + oracle::step(c),
                Ea::Displacement | Ea::Index => ea + 16,
                _ => ea,
            };
            c.r.set_address_register(usize::from(c.register), base);
            c.extension = if c.form.ea == Ea::Index {
                0x60f0
            } else {
                0xfff0
            };
            if c.form.ea == Ea::Index {
                c.r.d[6] = 0;
            }
        }
        Ea::AbsoluteWord => c.extension = ea as u16,
        Ea::AbsoluteLong => {
            c.extension = 0;
            c.low = ea as u16;
        }
    }
    c
}

pub(super) const WORDS: [u16; 66] = [
    0x0c00, 0x0c01, 0x0c02, 0x0c03, 0x0c04, 0x0c05, 0x0c06, 0x0c07, 0x0c40, 0x0c41, 0x0c42, 0x0c43,
    0x0c44, 0x0c45, 0x0c46, 0x0c47, 0x0c80, 0x0c81, 0x0c82, 0x0c83, 0x0c84, 0x0c85, 0x0c86, 0x0c87,
    0x0c10, 0x0c11, 0x0c12, 0x0c13, 0x0c14, 0x0c15, 0x0c16, 0x0c17, 0x0c18, 0x0c19, 0x0c1a, 0x0c1b,
    0x0c1c, 0x0c1d, 0x0c1e, 0x0c1f, 0x0c20, 0x0c21, 0x0c22, 0x0c23, 0x0c24, 0x0c25, 0x0c26, 0x0c27,
    0x0c28, 0x0c29, 0x0c2a, 0x0c2b, 0x0c2c, 0x0c2d, 0x0c2e, 0x0c2f, 0x0c30, 0x0c31, 0x0c32, 0x0c33,
    0x0c34, 0x0c35, 0x0c36, 0x0c37, 0x0c38, 0x0c39,
];
pub(super) const REPRESENTATIVES: [u16; 10] = [
    0x0c00, 0x0c40, 0x0c80, 0x0c17, 0x0c1f, 0x0c27, 0x0c2f, 0x0c37, 0x0c38, 0x0c39,
];

pub(super) fn coverage() -> (u32, u32) {
    let mut bad = 0;
    for bank in [false, true] {
        for word in WORDS {
            bad += u32::from(!run::check(case(word, bank, u32::from(word & 1)), None, 3));
        }
    }
    (132, bad)
}

pub(super) fn first() -> (u32, u32) {
    let (mut count, mut bad) = coverage();
    for bank in [false, true] {
        for word in REPRESENTATIVES {
            let c = case(word, bank, 1);
            let (_, b) = c.seed();
            let plan = oracle::plan(c, &b.words);
            count += 1;
            bad +=
                u32::from(!super::remainder_literals::check(c, &plan) || !run::check(c, None, 0));
            for at in 0..plan.len() {
                for kind in [Failure::Host, Failure::Unavailable, Failure::Boundary] {
                    count += 1;
                    bad += u32::from(!run::check(c, Some((at, kind)), 3));
                }
            }
        }
    }
    (count, bad)
}

pub(super) fn migrated(word: u16, old: Case) -> bool {
    let (_, bus) = old.seed();
    let mut c = case(word, old.r.supervisor(), 0);
    c.r = old.r;
    c.start = old.start;
    c.immediate = u32::from(old.retained()) << if c.form.size == Size::Long { 16 } else { 0 };
    let cpu =
        PrefetchCpu::from_prefetched(c.r, [word, old.retained()], CpuClocks(c.start)).unwrap();
    run::seeded(c, cpu, bus)
}

pub(super) fn capacity() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for bank in [false, true] {
        for word in REPRESENTATIVES {
            for blocked in 0..case(word, bank, 1).form.count() {
                let mut c = case(word, bank, 1);
                let idle = c.form.idle() && blocked >= c.form.immediates();
                c.start =
                    u64::MAX - u64::from(u32::MAX) + 1 - blocked as u64 * 4 - u64::from(idle) * 2;
                let (mut cpu, mut b) = c.seed();
                let old = b.words.clone();
                let plan = oracle::plan(c, &old);
                let mut events = oracle::events(c, &plan[..blocked], &b.elapsed, true);
                let clock = c.start + blocked as u64 * 4 + u64::from(idle) * 2;
                if idle && blocked == c.form.immediates() {
                    events.push(Event::Idle(clock - 2, 2));
                }
                let r = if blocked == 0 {
                    c.r
                } else {
                    plan[blocked - 1].registers
                };
                let result = cpu.step_physical_instruction(&mut b);
                count += 1;
                bad += u32::from(
                    !(result == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                        && cpu.registers == r
                        && cpu.queued_words() == Some([word, c.retained()])
                        && cpu.clock().0 == clock
                        && b.clock == clock
                        && b.words == old
                        && b.events == events
                        && b.count == blocked
                        && b.entry_count == 0
                        && run::frozen(&mut cpu, &mut b, TimingFault::Frozen)),
                );
            }
        }
        for room in 0..=2 {
            let mut c = case(0x0c80, bank, 0);
            c.start = u64::MAX - u64::from(u32::MAX) - 8 - room;
            let (mut cpu, mut b) = c.seed();
            b.elapsed[2] = u32::MAX;
            let old = b.words.clone();
            let plan = oracle::plan(c, &old);
            let mut events = oracle::events(c, &plan, &b.elapsed, true);
            let mut r = plan[2].registers;
            let good = if room == 2 {
                events.push(Event::Idle(u64::MAX - 2, 2));
                r.pc = 0x1006;
                cpu.step_physical_instruction(&mut b)
                    == Ok(PhysicalInstructionStep::Instruction(
                        zeff_m68k::timing::TimedStep {
                            opcode: 0x0c80,
                            clocks: u64::MAX - c.start,
                            exception: None,
                        },
                    ))
                    && cpu.queued_words() == Some([0x4afc, 0x7201])
            } else {
                cpu.step_physical_instruction(&mut b)
                    == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                    && cpu.queued_words() == Some([0x0c80, 0x8000])
                    && run::frozen(&mut cpu, &mut b, TimingFault::Frozen)
            };
            count += 1;
            bad += u32::from(
                !(good
                    && cpu.registers == r
                    && b.words == old
                    && b.events == events
                    && b.count == 3
                    && b.entry_count == 0
                    && cpu.clock().0 == b.clock
                    && b.clock == u64::MAX - if room == 2 { 0 } else { room }),
            );
        }
    }
    (count, bad)
}
