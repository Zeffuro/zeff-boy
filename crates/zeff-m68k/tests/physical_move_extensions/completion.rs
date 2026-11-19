use super::*;

fn complete(case: Case, waits: bool) {
    let (mut cpu, mut bus, case) = case.seed();
    if waits {
        bus.elapsed = [7, 11, 5, 13, 9];
    }
    check(&mut cpu, &mut bus, case);
}

fn check(cpu: &mut PrefetchCpu, bus: &mut Device, case: Case) {
    let plan = oracle::plan(case, &bus.words);
    let memory = oracle::memory(bus.words.clone(), &plan);
    let last = plan.last().unwrap();
    let clocks = oracle::clocks(case, &bus.elapsed, plan.len());
    assert_eq!(
        cpu.step_physical_instruction(bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: case.form.opcode(case.dn, case.an),
                clocks,
                exception: None
            }
        ))
    );
    let mut expected = last.registers;
    expected.pc = case.r.pc.wrapping_add(case.form.length());
    assert_eq!(cpu.registers, expected);
    assert_eq!(
        cpu.queued_words(),
        Some([
            last.ird,
            oracle::sample(&memory, last.access.logical_address)
        ])
    );
    assert_eq!(bus.words, memory);
    assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed));
    assert_eq!(bus.count, case.form.count());
    assert_eq!(bus.entry_count, 0);
    assert_eq!(cpu.clock().0, case.start + clocks);
    assert_eq!(cpu.clock().0, bus.clock);
    assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
    assert_eq!(
        cpu.physical_inspection(),
        PhysicalInspection::DiagnosticCompleteValues
    );
}

#[test]
fn all_936_encodings_complete_in_both_banks_with_x_waits_and_byte_lanes() {
    let mut encoding = BTreeMap::new();
    let mut executions = 0;
    for form in forms() {
        for dn in 0..8 {
            for an in 0..if form.uses_an() { 8 } else { 1 } {
                assert!(encoding.insert(form.opcode(dn, an), ()).is_none());
                for supervisor in [false, true] {
                    for ccr in [0, 31] {
                        for waits in [false, true] {
                            for odd in 0..if form.size == Size::Byte && form.memory() {
                                2
                            } else {
                                1
                            } {
                                let mut case = Case::new(form);
                                case.r = registers(supervisor, ccr);
                                case.dn = dn;
                                case.an = an;
                                case.e0 = case.e0.wrapping_add(odd);
                                if form.family == Family::AbsoluteLong {
                                    case.e4 += odd;
                                }
                                complete(case, waits);
                                executions += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(forms().len(), 33);
    assert_eq!(encoding.len(), 936);
    assert_eq!(executions, 9920);
}

#[test]
fn index_uses_old_data_or_banked_address_word_or_long_and_ignores_later_features() {
    for form in forms().into_iter().filter(|f| f.indexed()) {
        for supervisor in [false, true] {
            for address in [false, true] {
                for long in [false, true] {
                    for reg in 0..8 {
                        for later in [0, 0x0700] {
                            let mut case = Case::new(form);
                            case.r = registers(supervisor, 31);
                            case.dn = reg;
                            case.r.d[usize::from(reg)] = 0xffff_8000;
                            case.value = 0xffff_8000;
                            case.r.a[usize::from(reg.min(6))] = 0xfedc_8000;
                            case.r.usp = 0x7654_8000;
                            case.r.ssp = 0x8765_8000;
                            case.e0 = (u16::from(address) << 15)
                                | reg << 12
                                | (u16::from(long) << 11)
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
fn logical_and_external_wrap_negative_absolute_and_code_alias_preserve_actual_next() {
    for form in forms() {
        for supervisor in [false, true] {
            let mut case = Case::new(form);
            case.r = registers(supervisor, 16);
            case.r.pc = 0xffff_fffc;
            match form.family {
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
                Family::Immediate => case.e0 = 0x1234,
            }
            complete(case, true);
        }
    }
    for form in forms().into_iter().filter(|f| f.write) {
        let mut case = Case::new(form);
        let target = case.r.pc.wrapping_add(form.length());
        match form.family {
            Family::Displacement => {
                case.e0 = 0;
                case.r.a[2] = target;
            }
            Family::Indexed => {
                case.e0 = 0x4800;
                case.r.d[4] = 0;
                case.r.a[2] = target;
            }
            Family::AbsoluteWord => {
                case.r.pc = 0x2000;
                case.e0 = (0x2000 + form.length()) as u16;
            }
            Family::AbsoluteLong => {
                case.e0 = (target >> 16) as u16;
                case.e4 = target as u16;
            }
            _ => unreachable!(),
        }
        complete(case, true);
    }
}

#[test]
fn actual_mutated_e4_drives_absolute_ea_and_long_immediate_low_half() {
    for form in forms().into_iter().filter(|f| {
        f.family == Family::AbsoluteLong || f.family == Family::Immediate && f.size == Size::Long
    }) {
        let (mut cpu, mut bus, mut case) = Case::new(form).seed();
        case.e4 = 0x7000;
        if form.memory() {
            bus.operand(case.effective(case.e4), form.size, case.value);
        }
        bus.mutate_read = Some((case.r.pc + 4, case.e4));
        let mut image = bus.words.clone();
        image.insert((case.r.pc + 4) & BUS_ADDRESS_MASK, case.e4);
        let plan = oracle::plan(case, &image);
        let memory = oracle::memory(image, &plan);
        let result = cpu.step_physical_instruction(&mut bus);
        assert!(matches!(
            result,
            Ok(PhysicalInstructionStep::Instruction(_))
        ));
        let mut expected = plan.last().unwrap().registers;
        expected.pc += form.length();
        assert_eq!(cpu.registers, expected);
        assert_eq!(bus.words, memory);
        assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed));
        assert_eq!(cpu.queued_words(), Some([case.next, case.last]));
        assert_eq!(cpu.clock().0, bus.clock);
        assert!(bus.mutate_read.is_none());
    }
}
