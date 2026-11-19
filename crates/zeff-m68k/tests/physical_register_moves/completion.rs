use super::*;

fn completed(case: Case, waited: bool) {
    let (mut cpu, mut bus, case) = case.seed();
    if waited {
        bus.elapsed = [7, 11, 5, 13, 9];
    }
    let memory = bus.words.clone();
    let plan = oracle::plan(case, &memory);
    let clocks = oracle::clocks(case, &bus.elapsed, plan.len());
    let next = if case.form.length() == 2 {
        case.e0
    } else {
        bus.sample(case.r.pc.wrapping_add(case.form.length()))
    };
    let last = oracle::fetched(memory.clone(), &plan);
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
    assert_eq!(cpu.queued_words(), Some([next, last]));
    assert_eq!(bus.words, oracle::memory(memory, &plan));
    assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed));
    assert_eq!(bus.count, plan.len());
    assert_eq!(bus.entry_count, 0);
    assert_eq!(cpu.clock().0, case.start + clocks);
    assert_eq!(cpu.clock().0, bus.clock);
}

#[test]
fn all_992_encodings_preserve_banks_lanes_source_snapshots_flags_and_waits() {
    let mut encodings = std::collections::BTreeSet::new();
    let mut executions = 0;
    for form in forms() {
        for source in 0..8 {
            for destination in 0..if form.variable() { 8 } else { 1 } {
                encodings.insert(form.opcode(destination, source));
                for supervisor in [false, true] {
                    for value in [0, 0x0001_0000, 0xffff_8000, 0x8000_0001] {
                        for waited in [false, true] {
                            let mut case = Case::new(form);
                            case.r = registers(supervisor, 31);
                            case.source = source;
                            case.destination = destination;
                            if form.family == Family::Data {
                                case.r.d[usize::from(source)] = value;
                            } else {
                                case.r.set_address_register(usize::from(source), value);
                            }
                            if !form.direct()
                                && form.variable()
                                && source == destination
                                && value & 1 != 0
                            {
                                continue;
                            }
                            completed(case, waited);
                            executions += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(forms().len(), 19);
    assert_eq!(encodings.len(), 992);
    assert_eq!(executions, 15_552);
}

#[test]
fn every_ccr_pattern_and_even_aliased_source_base_index_is_preserved_at_completion() {
    for form in forms() {
        for ccr in 0..32 {
            for supervisor in [false, true] {
                let mut case = Case::new(form);
                case.r = registers(supervisor, ccr);
                case.source = 7;
                case.destination = 7;
                case.e0 = if form.family == Family::Indexed {
                    0xfef0
                } else {
                    case.e0
                };
                completed(case, true);
            }
        }
    }
}

#[test]
fn word_move_an_to_dn_merges_instead_of_sign_extending_and_odd_source_values_are_legal() {
    for supervisor in [false, true] {
        for size in [Size::Word, Size::Long] {
            let form = Form {
                size,
                family: Family::Address,
            };
            let mut case = Case::new(form);
            case.r = registers(supervisor, 31);
            case.source = 7;
            case.destination = 1;
            case.r.d[1] = 0x1234_5678;
            case.r.set_address_register(7, 0xffff_8001);
            let (mut cpu, mut bus, _) = case.seed();
            assert!(matches!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Instruction(_))
            ));
            assert_eq!(
                cpu.registers.d[1],
                if size == Size::Word {
                    0x1234_8001
                } else {
                    0xffff_8001
                }
            );
            assert_eq!(bus.count, 1);
            assert_eq!(cpu.clock().0, 41);
            assert_eq!(cpu.registers.usp, case.r.usp);
            assert_eq!(cpu.registers.ssp, case.r.ssp);
        }
    }
}

#[test]
fn every_index_register_bank_width_and_ignored_extension_bits_uses_old_state() {
    for size in [Size::Word, Size::Long] {
        for supervisor in [false, true] {
            for address in [false, true] {
                for long in [false, true] {
                    for index in 0..8 {
                        let mut case = Case::new(Form {
                            size,
                            family: Family::Indexed,
                        });
                        case.r = registers(supervisor, 31);
                        case.source = index;
                        case.destination = index;
                        case.e0 = (if address { 0x8000 } else { 0 })
                            | index << 12
                            | if long { 0x800 } else { 0 }
                            | 0x6f0;
                        completed(case, true);
                    }
                }
            }
        }
    }
}

#[test]
fn full_logical_wrap_and_external_24_bit_wrap_keep_addresses_distinct() {
    for form in forms() {
        for pc in [0xffff_fffa, 0xffff_fffc, 0xffff_fffe, 0x12ff_fffa] {
            let mut case = Case::new(form);
            case.r.pc = pc;
            case.r.set_address_register(3, 0xffff_fffe);
            if form.family == Family::Displacement {
                case.e0 = 4;
            }
            if form.family == Family::Indexed {
                case.e0 = 0x4804;
                case.r.d[4] = 0;
            }
            completed(case, true);
        }
    }
}

#[test]
fn actual_e4_and_next_ram_mutations_drive_ea_and_retained_queue_without_expected_seeding() {
    for (form, at, actual) in [
        (
            Form {
                size: Size::Long,
                family: Family::AbsoluteLong,
            },
            4,
            0x6120,
        ),
        (
            Form {
                size: Size::Word,
                family: Family::Displacement,
            },
            4,
            0x7602,
        ),
        (
            Form {
                size: Size::Long,
                family: Family::AbsoluteLong,
            },
            6,
            0x7602,
        ),
    ] {
        let (mut cpu, mut bus, case) = Case::new(form).seed();
        bus.mutate_read = Some((case.r.pc + at, actual));
        let mut memory = bus.words.clone();
        memory.insert((case.r.pc + at) & BUS_ADDRESS_MASK, actual);
        let plan = oracle::plan(case, &memory);
        let last = oracle::fetched(memory.clone(), &plan);
        let next = oracle::sample(&memory, case.r.pc + form.length());
        assert!(matches!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Instruction(_))
        ));
        assert_eq!(bus.words, oracle::memory(memory, &plan));
        assert_eq!(cpu.queued_words(), Some([next, last]));
        assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed));
    }
}
