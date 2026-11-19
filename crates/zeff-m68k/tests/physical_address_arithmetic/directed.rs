use super::*;

fn target(case: &mut Case, address: u32) {
    case.register = 0;
    match case.form.ea {
        Ea::Indirect | Ea::Post => {
            case.r.set_address_register(0, address);
        }
        Ea::Pre => {
            case.r
                .set_address_register(0, address.wrapping_add(case.form.stride()));
        }
        Ea::Displacement => {
            case.extension = 0;
            case.r.set_address_register(0, address);
        }
        Ea::Index => {
            case.extension = 0x4800;
            case.r.d[4] = 0;
            case.r.set_address_register(0, address);
        }
        Ea::PcDisplacement => {
            let delta = address.wrapping_sub(case.r.pc.wrapping_add(2));
            assert_eq!(i32::from(delta as i16) as u32, delta);
            case.extension = delta as u16;
        }
        Ea::PcIndex => {
            case.extension = 0x4800;
            case.r.d[4] = address.wrapping_sub(case.r.pc.wrapping_add(2));
        }
        Ea::AbsoluteWord => {
            assert_eq!(i32::from(address as i16) as u32, address);
            case.extension = address as u16;
        }
        Ea::AbsoluteLong => {
            case.extension = (address >> 16) as u16;
            case.low = address as u16;
        }
    }
}

#[test]
fn full_pc_wrap_and_operand_ea_wrap_are_disjoint_and_keep_24_bit_bus_aliases() {
    let mut rows = 0;
    for form in forms() {
        for pc in [0xffff_fffc, 0xffff_fffe] {
            let mut case = Case::new(form);
            case.r.pc = pc;
            let (_, bus) = case.seed();
            completion::inputs(case, &bus);
            let ea = oracle::address(case, &bus.words);
            assert_ne!(ea & BUS_ADDRESS_MASK, pc & BUS_ADDRESS_MASK);
            oracle::completed(case, true);
            for at in 0..form.count() {
                capture::failed(case, at, Failure::Boundary);
            }
            rows += 1;
        }
        for ea in [0xffff_fffe, 0x00ff_fffe] {
            if form.ea == Ea::AbsoluteWord && ea != 0xffff_fffe {
                continue;
            }
            let mut case = Case::new(form);
            case.r.pc = if form.ea == Ea::PcDisplacement {
                if ea == 0xffff_fffe {
                    0xffff_e000
                } else {
                    0x00ff_e000
                }
            } else {
                0x2000
            };
            target(&mut case, ea);
            let (_, bus) = case.seed();
            assert_eq!(oracle::address(case, &bus.words), ea);
            completion::inputs(case, &bus);
            let plan = oracle::plan(case, &bus.words);
            assert_eq!(plan[form.extensions()].access.logical_address, ea);
            if form.size == Size::Long {
                assert_eq!(
                    plan[form.extensions() + 1].access.logical_address,
                    ea.wrapping_add(2)
                );
                assert_eq!(plan[form.extensions() + 1].access.bus_address, 0);
            }
            oracle::completed(case, true);
            for at in 0..form.count() {
                capture::failed(case, at, Failure::Boundary);
            }
            rows += 1;
        }
    }
    assert_eq!(rows, 140);
}

#[test]
fn source_code_overlaps_keep_retained_extensions_and_use_actual_n_and_f_ram() {
    for form in forms() {
        for supervisor in [false, true] {
            let mut positions: Vec<_> = if form.ea == Ea::AbsoluteLong {
                vec![4, 6]
            } else {
                vec![2, form.length(), form.length() + 2]
            };
            positions.dedup();
            for offset in positions {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.r.pc = 0x1000;
                let address = case.r.pc + offset;
                target(&mut case, address);
                case.value = if form.ea == Ea::AbsoluteLong && offset == 4 {
                    if form.size == Size::Long {
                        0x1004_4a45
                    } else {
                        0x1004
                    }
                } else if form.size == Size::Long {
                    0x4a45_4a45
                } else {
                    0x4a45
                };
                let (_, bus) = case.seed();
                assert_eq!(oracle::address(case, &bus.words), address);
                completion::inputs(case, &bus);
                let next = oracle::next(case, &bus.words);
                if form.extensions() == 0 {
                    assert_eq!(next, 0x7201);
                } else if offset == form.length() {
                    assert_eq!(next, 0x4a45);
                }
                oracle::completed(case, true);
                for at in 0..form.count() {
                    capture::failed(case, at, Failure::Boundary);
                }
            }
        }
    }
}

#[test]
fn source_and_code_overlapping_exception_stack_are_read_before_frame_mutations() {
    for form in forms() {
        for code in [false, true] {
            let mut case = Case::new(form);
            case.r = registers(true, 31);
            case.r.pc = 0x4000;
            case.r.ssp = if code {
                case.r.pc + form.length() + 2
            } else {
                0x8000
            };
            if !code {
                let address = case.r.ssp - 14;
                target(&mut case, address);
            }
            let (mut cpu, mut bus) = case.seed();
            let initial = bus.words.clone();
            let plan = oracle::plan(case, &initial);
            let last = plan.len() - 1;
            let t = plan[last];
            let c = oracle::capture(t, 2);
            bus.failure = Some((last, Failure::Boundary));
            let clocks = oracle::clocks(case, &bus.elapsed, plan.len(), true);
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Group0 {
                    opcode: case.opcode(),
                    capture: c,
                    clocks: clocks + 50
                })
            );
            assert_eq!(cpu.registers, oracle::entered(t.registers));
            assert_eq!(bus.words, oracle::entry_memory(initial, t.registers.ssp, c));
            oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn group0_ssp_wrap_keeps_full_logical_bank_and_wrapped_frame_bus_addresses() {
    for form in forms() {
        for supervisor in [false, true] {
            for sp in [2, 0x0100_0002] {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.r.ssp = sp;
                case.register = 0;
                let (_, bus) = case.seed();
                completion::inputs(case, &bus);
                capture::failed(case, 0, Failure::Boundary);
                capture::failed(case, form.count() - 1, Failure::Boundary);
            }
        }
    }
}

fn mutated_completion(
    case: Case,
    cpu: &mut PrefetchCpu,
    bus: &mut BusDevice,
    shadow: BTreeMap<u32, u16>,
) {
    let plan = oracle::plan(case, &shadow);
    let tail = if case.form.size == Size::Word { 4 } else { 2 };
    let clocks = oracle::clocks(case, &bus.elapsed, plan.len(), true) + tail;
    assert_eq!(
        cpu.step_physical_instruction(bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: case.opcode(),
                clocks,
                exception: None
            }
        ))
    );
    let mut r = plan.last().unwrap().registers;
    r.set_address_register(
        usize::from(case.destination),
        oracle::arithmetic(case, &shadow).1,
    );
    r.pc = case.r.pc.wrapping_add(case.form.length());
    assert_eq!(cpu.registers, r);
    assert_eq!(
        cpu.queued_words(),
        Some([
            oracle::next(case, &shadow),
            oracle::sample(&shadow, r.pc.wrapping_add(2))
        ])
    );
    assert_eq!(bus.words, shadow);
    let mut events = oracle::events(case, &plan, &bus.elapsed, true);
    if tail == 4 {
        events.push(Event::Idle(case.start + clocks - 4, 2));
    }
    events.push(Event::Idle(case.start + clocks - 2, 2));
    assert_eq!(bus.events, events);
    assert_eq!(cpu.clock().0, case.start + clocks);
    assert_eq!(cpu.clock().0, bus.clock);
}

#[test]
fn actual_abs_long_low_read_changes_ea_before_operand_callbacks() {
    for (operation, supervisor) in
        [Operation::Add, Operation::Subtract]
            .into_iter()
            .flat_map(|operation| {
                [false, true]
                    .into_iter()
                    .map(move |supervisor| (operation, supervisor))
            })
    {
        let mut case = Case::new(Form {
            operation,
            size: Size::Long,
            ea: Ea::AbsoluteLong,
        });
        case.r = registers(supervisor, 31);
        case.value = 0x0001_0000;
        let (mut cpu, mut bus) = case.seed();
        bus.operand(0xabcd_6200, Size::Long, 0x8000_0001);
        bus.mutate_read = Some((case.r.pc + 4, 0x6200));
        let mut shadow = bus.words.clone();
        shadow.insert((case.r.pc + 4) & BUS_ADDRESS_MASK, 0x6200);
        assert_eq!(oracle::address(case, &shadow), 0xabcd_6200);
        assert_eq!(oracle::value(case, &shadow), 0x8000_0001);
        mutated_completion(case, &mut cpu, &mut bus, shadow);
    }
}

#[test]
fn actual_retained_n_operand_high_low_and_final_fetch_reads_can_change_in_ram() {
    for (operation, supervisor) in
        [Operation::Add, Operation::Subtract]
            .into_iter()
            .flat_map(|operation| {
                [false, true]
                    .into_iter()
                    .map(move |supervisor| (operation, supervisor))
            })
    {
        for which in 0..4 {
            let mut case = Case::new(Form {
                operation,
                size: Size::Long,
                ea: Ea::Displacement,
            });
            case.r = registers(supervisor, 31);
            let (mut cpu, mut bus) = case.seed();
            let source = oracle::address(case, &bus.words);
            let (address, value) = match which {
                0 => (case.r.pc + case.form.length(), 0x4a45),
                1 => (source, 0x0001),
                2 => (source.wrapping_add(2), 0xffff),
                _ => (case.r.pc + case.form.length() + 2, 0x6d5f),
            };
            bus.mutate_read = Some((address, value));
            let mut shadow = bus.words.clone();
            shadow.insert(address & BUS_ADDRESS_MASK & !1, value);
            assert_eq!(oracle::address(case, &shadow), source);
            mutated_completion(case, &mut cpu, &mut bus, shadow);
        }
    }
}

#[test]
fn changed_actual_extension_operand_n_and_f_reads_keep_exact_consumed_fault_prefix() {
    let mut rows = 0;
    for (operation, size) in
        [Operation::Add, Operation::Subtract]
            .into_iter()
            .flat_map(|operation| {
                [Size::Word, Size::Long]
                    .into_iter()
                    .map(move |size| (operation, size))
            })
    {
        for supervisor in [false, true] {
            for which in 0..5 {
                let form = Form {
                    operation,
                    size,
                    ea: if which == 0 {
                        Ea::AbsoluteLong
                    } else {
                        Ea::PcDisplacement
                    },
                };
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                completion::pair(&mut case, completion::pairs(size)[3]);
                for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                    let (cpu, mut bus) = case.seed();
                    completion::inputs(case, &bus);
                    let source = oracle::address(case, &bus.words);
                    let (address, value, at) = match which {
                        0 => {
                            bus.operand(
                                0xabcd_6200,
                                size,
                                if size == Size::Long {
                                    0x8000_0001
                                } else {
                                    0x8001
                                },
                            );
                            (case.r.pc + 4, 0x6200, 0)
                        }
                        1 => (source, 0x8000, form.extensions()),
                        2 if size == Size::Long => (source + 2, 0x8001, form.extensions() + 1),
                        2 => (source, 0xffff, form.extensions()),
                        3 => (case.r.pc + form.length(), 0xb29f, form.extensions() - 1),
                        _ => (case.r.pc + form.length() + 2, 0x1234, form.count() - 1),
                    };
                    bus.mutate_read = Some((address, value));
                    let mut shadow = bus.words.clone();
                    shadow.insert(address & BUS_ADDRESS_MASK & !1, value);
                    let actual_ea = oracle::address(case, &shadow);
                    assert_eq!(actual_ea & 1, 0);
                    assert_eq!(actual_ea, if which == 0 { 0xabcd_6200 } else { source });
                    capture::failed_seeded(case, at, failure, cpu, bus, shadow);
                    rows += 1;
                }
            }
        }
    }
    assert_eq!(rows, 120);
}
