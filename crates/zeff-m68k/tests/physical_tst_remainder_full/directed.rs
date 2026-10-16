use super::*;
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    match index {
        13 => {
            for mut s in model::encoded() {
                for pc in [
                    0x2000,
                    0xffff_fffa,
                    0xffff_fffc,
                    0xffff_fffe,
                    0x00ff_fffc,
                    0x00ff_fffe,
                ] {
                    for path in 0..4 {
                        s.pc = pc;
                        tally.add(check::run(
                            s,
                            |b| {
                                if path != 0 {
                                    let (k, n) = model::REPORTS[path - 1];
                                    b.report = Some((s.count() - 1, k, n));
                                }
                            },
                            mark,
                        ));
                    }
                }
            }
        }
        14 => {
            for mut s in model::encoded().filter(|s| s.mode() != 0) {
                for ea in [
                    0,
                    1,
                    0x00ff_fffe,
                    0x00ff_ffff,
                    0xffff_fffe,
                    0xffff_ffff,
                    0x8000_0000,
                    0x8000_0001,
                ] {
                    for path in 0..4 {
                        s.desired = ea;
                        s.value = 0x80;
                        s.lane = ea & 1;
                        tally.add(check::run(
                            s,
                            |b| {
                                if path != 0 {
                                    let (k, n) = model::REPORTS[path - 1];
                                    b.report = Some((s.count() - 1, k, n));
                                }
                            },
                            mark,
                        ));
                    }
                }
            }
        }
        15 => {
            for (s, at) in model::triggers() {
                let (_, _, b, _, _) = s.seed();
                let e = oracle::predict(s, &b);
                let reads: Vec<_> = e
                    .events
                    .iter()
                    .filter_map(|event| {
                        if let device::Event::Transfer(
                            device::Stage::Instruction,
                            a,
                            _,
                            _,
                            _,
                            _,
                            _,
                            _,
                        ) = event
                        {
                            Some(*a)
                        } else {
                            None
                        }
                    })
                    .collect();
                for target in 0..3 {
                    let address = match target {
                        0 => reads[at].logical_address,
                        1 => reads
                            .iter()
                            .skip(at + 1)
                            .copied()
                            .find(|a| a.kind == AccessKind::Prefetch)
                            .map(|a| a.logical_address)
                            .unwrap_or(s.anchor().wrapping_add(4)),
                        _ => s.pc.wrapping_add(2),
                    };
                    for path in 0..4 {
                        tally.add(check::run(
                            s,
                            |b| {
                                b.mutation = Some((at, address, 0x4a45));
                                if path != 0 {
                                    let (k, n) = model::REPORTS[path - 1];
                                    b.report = Some((at, k, n));
                                }
                            },
                            mark,
                        ));
                    }
                }
            }
        }
        16 => {
            for s in model::encoded() {
                for route in 0..6 {
                    for path in 0..4 {
                        let mut row = s;
                        match route {
                            0..=2 => {
                                let address =
                                    [s.pc, s.pc.wrapping_add(2), s.anchor().wrapping_add(2)][route];
                                if s.mode() == 0 {
                                    row.ssp = address.wrapping_add(14);
                                } else {
                                    row.desired = address;
                                }
                            }
                            3 => row.ssp = s.pc.wrapping_add(14),
                            4 => row.pc = 8,
                            5 => row.pc = 0x4000,
                            _ => unreachable!(),
                        };
                        tally.add(check::run(
                            row,
                            |b| {
                                if path != 0 {
                                    let (k, n) = model::REPORTS[path - 1];
                                    b.report = Some((row.count() - 1, k, n));
                                }
                            },
                            mark,
                        ));
                    }
                }
            }
        }
        17 => {
            for (mut s, at) in model::triggers() {
                for nword in [0x4e71, 0x4a45, 0xffff] {
                    s.nword = nword;
                    for (k, n) in model::REPORTS {
                        tally.add(check::run(s, |b| b.report = Some((at, k, n)), mark));
                    }
                }
            }
        }
        31 => {
            for mut s in model::encoded() {
                for mask in 0..8 {
                    for route in 0..4 {
                        s.mask = mask;
                        s.nword = 0x4e71;
                        s.fword = 0x7002;
                        tally.add(next(s, route, mark));
                    }
                }
            }
        }
        _ => unreachable!(),
    }
    tally.result()
}
fn next(s: model::Spec, route: u8, mark: fn(bool)) -> bool {
    let (_, _, config, _, _) = s.seed();
    let e = oracle::predict(s, &config);
    let (level, edge) = match route {
        0 => (0, false),
        1 => (s.mask as u8, false),
        2 if s.mask < 7 => (s.mask as u8 + 1, false),
        _ => (0, true),
    };
    let latched_edge = edge || level == 7;
    let accepted = latched_edge || u16::from(level) > s.mask;
    let mut final_e = e.clone();
    final_e.level = level;
    final_e.edge = latched_edge;
    let expected = if accepted {
        Err(InstructionFault::InterruptUnqualified)
    } else {
        let address = e.r.pc.wrapping_add(4);
        let access = model::access(address, if s.bank { 6 } else { 2 }, AccessKind::Prefetch);
        let value = e.words.get(&access.bus_address).copied().unwrap_or(0);
        final_e.events.push(device::Event::Transfer(
            device::Stage::Instruction,
            access,
            Direction::Read,
            value,
            e.clock,
            4,
            device::Kind::Complete,
            false,
        ));
        final_e
            .scripts
            .push((device::Stage::Instruction, device::Kind::Complete, 4));
        final_e.count += 1;
        final_e.clock += 4;
        final_e.r.pc = e.r.pc.wrapping_add(2);
        final_e.queue = Some([s.fword, value]);
        Ok(PhysicalInstructionStep::Instruction(TimedStep {
            opcode: 0x4e71,
            clocks: 4,
            exception: None,
        }))
    };
    mark(true);
    let (r, q, mut b, _, _) = s.seed();
    let mut cpu = PrefetchCpu::from_prefetched(r, q, CpuClocks(0)).unwrap();
    let first = check::step(&mut cpu, &mut b);
    let saved = cpu.clone();
    let old = b.clone();
    check::interrupts(&mut cpu, level, edge);
    let actual = check::step(&mut cpu, &mut b);
    let copied = if accepted {
        check::rejected(&mut cpu, &mut b, InstructionFault::InterruptUnqualified)
    } else {
        true
    };
    mark(false);
    copied
        && first == e.result
        && check::verify(&saved, &old, &e, &config)
        && actual == expected
        && check::verify(&cpu, &b, &final_e, &config)
}
