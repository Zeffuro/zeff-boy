use super::*;

fn complete(case: Case, waits: bool) {
    let (mut cpu, mut bus, case) = case.seed();
    if waits {
        bus.elapsed = [7, 11, 5, 13, 9];
    }
    let plan = oracle::plan(case, &bus.words);
    let memory = bus.words.clone();
    let clocks = oracle::clocks(case, &bus.elapsed, plan.len());
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: case.form.opcode(case.destination, case.source),
                clocks,
                exception: None
            }
        ))
    );
    let mut expected = plan.last().unwrap().registers;
    expected.pc = case.r.pc.wrapping_add(case.form.length());
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.registers.d, case.r.d);
    assert_eq!(cpu.registers.status_register(), case.r.status_register());
    assert_eq!(
        cpu.queued_words(),
        Some([
            plan.last().unwrap().ird,
            bus.sample(plan.last().unwrap().access.logical_address)
        ])
    );
    assert_eq!(bus.words, memory);
    assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed));
    assert_eq!(bus.count, case.form.count());
    assert_eq!(bus.entry_count, 0);
    assert_eq!(cpu.clock().0, case.start + clocks);
    assert_eq!(cpu.clock().0, bus.clock);
    assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
}

#[test]
fn all_976_encodings_preserve_ccr_and_data_with_values_banks_waits_and_odd_results() {
    let mut encodings = BTreeMap::new();
    let mut executions = 0;
    for form in forms() {
        let values: &[u32] = if form.size == Size::Word {
            &[0, 0x7fff, 0x8000, 0xffff]
        } else {
            &[0x0001_0000, 0x8000_0001, u32::MAX]
        };
        for destination in 0..8 {
            for source in 0..if form.variable() { 8 } else { 1 } {
                assert!(
                    encodings
                        .insert(form.opcode(destination, source), ())
                        .is_none()
                );
                for supervisor in [false, true] {
                    for ccr in [0, 31] {
                        for &value in values {
                            for waits in [false, true] {
                                let mut case = Case::new(form);
                                case.r = registers(supervisor, ccr);
                                case.destination = destination;
                                case.source = source;
                                case.value = value;
                                complete(case, waits);
                                executions += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(forms().len(), 24);
    assert_eq!(encodings.len(), 976);
    assert_eq!(executions, 27328);
}

#[test]
fn base_index_destination_aliases_use_old_banked_word_or_long_index() {
    for form in forms()
        .into_iter()
        .filter(|f| matches!(f.family, Family::Indexed | Family::PcIndexed))
    {
        for supervisor in [false, true] {
            for address in [false, true] {
                for long in [false, true] {
                    for reg in 0..8 {
                        for later in [0, 0x0700] {
                            let mut case = Case::new(form);
                            case.r = registers(supervisor, 31);
                            case.source = reg;
                            case.destination = reg;
                            case.r.d[usize::from(reg)] = 0xffff_8000;
                            assert!(case.r.set_address_register(usize::from(reg), 0xfedc_8000));
                            case.e0 = u16::from(address) << 15
                                | reg << 12
                                | u16::from(long) << 11
                                | later
                                | 0xf0;
                            complete(case, true);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn post_and_pre_aliases_retain_original_operand_address_then_destination_overwrites_update() {
    for form in forms()
        .into_iter()
        .filter(|f| matches!(f.family, Family::Post | Family::Pre))
    {
        for supervisor in [false, true] {
            for reg in 0..8 {
                for value in [0x8000, 0x0001_0000, 0x8000_0001, u32::MAX] {
                    let mut case = Case::new(form);
                    case.r = registers(supervisor, 23);
                    case.destination = reg;
                    case.source = reg;
                    case.value = value;
                    complete(case, true);
                }
            }
        }
    }
}

#[test]
fn full_logical_wrap_and_negative_absolute_and_pc_relative_addresses_complete() {
    for form in forms() {
        let mut case = Case::new(form);
        case.r.pc = 0xffff_fffc;
        match form.family {
            Family::Indirect | Family::Post => case.r.a[2] = 0xffff_fffe,
            Family::Pre => case.r.a[2] = if form.size == Size::Long { 2 } else { 0 },
            Family::Displacement => {
                case.r.a[2] = 0;
                case.e0 = 0xfffe;
            }
            Family::Indexed => {
                case.r.a[2] = 0;
                case.r.d[4] = 0;
                case.e0 = 0x48fe;
            }
            Family::AbsoluteWord => case.e0 = 0xfffe,
            Family::AbsoluteLong => {
                case.e0 = 0xffff;
                case.e4 = 0xfffe;
            }
            Family::PcDisplacement => case.e0 = 0,
            Family::PcIndexed => {
                case.r.d[4] = 0;
                case.e0 = 0x4800;
            }
            _ => {}
        }
        complete(case, true);
    }
}

#[test]
fn actual_extension_and_next_callback_ram_returns_control_value_ea_and_queue() {
    for form in forms() {
        for next in [0x2970, 0x4afc] {
            let (mut cpu, mut bus, case) = Case::new(form).seed();
            if form.length() == 2 {
                continue;
            }
            let address = case.r.pc + form.length();
            bus.mutate_read = Some((address, next));
            let mut memory = bus.words.clone();
            memory.insert(address & BUS_ADDRESS_MASK, next);
            let plan = oracle::plan(case, &memory);
            assert!(matches!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Instruction(_))
            ));
            let mut expected = plan.last().unwrap().registers;
            expected.pc += form.length();
            assert_eq!(cpu.registers, expected);
            assert_eq!(bus.words, memory);
            assert_eq!(cpu.queued_words(), Some([next, case.last]));
            assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed));
            assert_eq!(cpu.clock().0, bus.clock);
            assert!(bus.mutate_read.is_none());
        }
    }
    for form in forms().into_iter().filter(|f| {
        f.family == Family::AbsoluteLong || f.family == Family::Immediate && f.size == Size::Long
    }) {
        for low in [0x7000, 0x7001] {
            let (mut cpu, mut bus, case) = Case::new(form).seed();
            if form.memory() {
                bus.operand(case.effective(low), form.size, case.value);
            }
            bus.mutate_read = Some((case.r.pc + 4, low));
            if form.memory() {
                bus.word(case.effective(low), 0x8000);
                bus.word(case.effective(low).wrapping_add(2), 1);
            }
            let mut memory = bus.words.clone();
            memory.insert((case.r.pc + 4) & BUS_ADDRESS_MASK, low);
            let plan = oracle::plan(case, &memory);
            if form.memory() && low & 1 != 0 {
                bus.detection = Some((Failure::Boundary, 17));
            }
            let result = cpu.step_physical_instruction(&mut bus);
            if form.memory() && low & 1 != 0 {
                let at = plan
                    .iter()
                    .position(|t| t.access.kind == AccessKind::Data)
                    .unwrap();
                let capture = oracle::capture(plan[at], 3);
                let clocks = oracle::clocks(case, &bus.elapsed, at) + 17;
                assert_eq!(
                    result,
                    Ok(PhysicalInstructionStep::Group0 {
                        opcode: form.opcode(case.destination, case.source),
                        capture,
                        clocks: clocks + 50
                    })
                );
                assert_eq!(capture.logical_address, case.effective(low));
                assert_eq!(cpu.registers, oracle::entered(plan[at].registers));
                assert_eq!(
                    bus.words,
                    oracle::entry_memory(memory, plan[at].registers.ssp, capture)
                );
            } else {
                assert!(matches!(
                    result,
                    Ok(PhysicalInstructionStep::Instruction(_))
                ));
                let mut expected = plan.last().unwrap().registers;
                expected.pc += form.length();
                assert_eq!(cpu.registers, expected);
                assert_eq!(bus.words, memory);
                assert_eq!(cpu.queued_words(), Some([case.next, case.last]));
            }
            assert_eq!(cpu.clock().0, bus.clock);
            assert!(bus.mutate_read.is_none());
        }
    }
}
