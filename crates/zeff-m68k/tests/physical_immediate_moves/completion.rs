use super::*;

pub(super) fn completed(case: Case, waited: bool) {
    let (mut cpu, mut bus, case) = case.seed();
    if waited {
        bus.elapsed = [7, 11, 5, 13, 9, 6, 8];
    }
    let memory = bus.words.clone();
    let plan = oracle::plan(case, &memory);
    let clocks = oracle::clocks(case, &bus.elapsed, plan.len(), true);
    let next = oracle::sample(&memory, case.r.pc.wrapping_add(case.form.length()));
    let last = oracle::fetched(memory.clone(), &plan);
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: case.form.opcode(case.destination),
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
    assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
    assert_eq!(bus.count, plan.len());
    assert_eq!(bus.entry_count, 0);
    assert_eq!(cpu.clock().0, case.start + clocks);
    assert_eq!(cpu.clock().0, bus.clock);
}

#[test]
fn all_126_encodings_and_21_forms_cover_ccr_banks_values_waits_and_byte_lanes() {
    let mut encodings = std::collections::BTreeSet::new();
    let mut executions = 0;
    for form in forms() {
        for destination in 0..if form.variable() { 8 } else { 1 } {
            encodings.insert(form.opcode(destination));
            for supervisor in [false, true] {
                for ccr in 0..32 {
                    for value in [0, 0x0001_0000, 0x0000_8000, 0x8000_0001] {
                        for waited in [false, true] {
                            for parity in 0..if form.size == Size::Byte { 2 } else { 1 } {
                                let mut case = Case::new(form);
                                case.r = registers(supervisor, ccr);
                                case.destination = destination;
                                case.value = value;
                                case.low = value as u16;
                                if form.size == Size::Byte {
                                    if form.variable() {
                                        let old = case
                                            .r
                                            .address_register(usize::from(destination))
                                            .unwrap();
                                        case.r.set_address_register(
                                            usize::from(destination),
                                            old | parity,
                                        );
                                    } else if form.family == Family::AbsoluteWord {
                                        case.extension |= parity as u16;
                                    } else {
                                        case.destination_low |= parity as u16;
                                    }
                                }
                                completed(case, waited);
                                executions += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(forms().len(), 21);
    assert_eq!(encodings.len(), 126);
    assert_eq!(executions, 86016);
}

#[test]
fn byte_negative_values_and_a7_stride_cover_both_lanes_without_touching_the_mate() {
    for family in [Family::Indirect, Family::Post, Family::Pre] {
        for destination in [0, 7] {
            for supervisor in [false, true] {
                for parity in [0, 1] {
                    let mut case = Case::new(Form {
                        size: Size::Byte,
                        family,
                    });
                    case.r = registers(supervisor, 31);
                    case.destination = destination;
                    case.value = 0x80;
                    case.r
                        .set_address_register(usize::from(destination), 0x1234_6000 | parity);
                    completed(case, true);
                }
            }
        }
    }
}

#[test]
fn every_d_a_word_long_index_selector_and_base_alias_uses_unchanged_registers() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        for supervisor in [false, true] {
            for address in [false, true] {
                for long in [false, true] {
                    for index in 0..8 {
                        let mut case = Case::new(Form {
                            size,
                            family: Family::Indexed,
                        });
                        case.r = registers(supervisor, 31);
                        case.destination = index;
                        case.extension = (if address { 0x8000 } else { 0 })
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
fn logical_32bit_and_external_24bit_wrap_preserve_source_and_destination_streams() {
    for form in forms() {
        for pc in [
            0xffff_fff6,
            0xffff_fffa,
            0xffff_fffc,
            0xffff_fffe,
            0x12ff_fffa,
        ] {
            let mut case = Case::new(form);
            case.r.pc = pc;
            case.r.set_address_register(3, 0xffff_fffe);
            if form.family == Family::Displacement {
                case.extension = 4;
            }
            if form.family == Family::Indexed {
                case.extension = 0x4804;
                case.r.d[4] = 0;
            }
            completed(case, true);
        }
    }
}

#[test]
fn actual_source_low_destination_extensions_next_and_final_fetch_are_read_from_ram() {
    for (size, family, offset, actual) in [
        (Size::Long, Family::Indirect, 4, 0x8000),
        (Size::Long, Family::Displacement, 6, 0xfff2),
        (Size::Long, Family::AbsoluteLong, 8, 0x6120),
        (Size::Long, Family::AbsoluteLong, 10, 0x7602),
        (Size::Word, Family::Displacement, 4, 0xfff2),
        (Size::Word, Family::AbsoluteLong, 6, 0x6120),
        (Size::Word, Family::AbsoluteLong, 8, 0x7602),
        (Size::Long, Family::Indirect, 8, 0x7680),
    ] {
        let (mut cpu, mut bus, case) = Case::new(Form { size, family }).seed();
        let address = case.r.pc.wrapping_add(offset);
        bus.mutate_read = Some((address, actual));
        let mut memory = bus.words.clone();
        memory.insert(address & BUS_ADDRESS_MASK & !1, actual);
        let plan = oracle::plan(case, &memory);
        let next = oracle::sample(&memory, case.r.pc + case.form.length());
        let last = oracle::fetched(memory.clone(), &plan);
        assert!(matches!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Instruction(_))
        ));
        let mut expected = plan.last().unwrap().registers;
        expected.pc = case.r.pc + case.form.length();
        assert_eq!(cpu.registers, expected);
        assert_eq!(bus.words, oracle::memory(memory, &plan));
        assert_eq!(cpu.queued_words(), Some([next, last]));
        assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
        assert_eq!(cpu.clock().0, bus.clock);
    }
}

#[test]
fn overlapping_code_stores_retain_source_low_and_promoted_next_and_early_f() {
    for form in forms() {
        for overlap in [4, form.length()] {
            let mut case = Case::new(form);
            let ea = case.r.pc + overlap;
            match form.family {
                Family::Indirect | Family::Post => {
                    case.r.set_address_register(3, ea);
                }
                Family::Pre => {
                    case.r.set_address_register(3, ea + form.stride(3));
                }
                Family::Displacement => {
                    case.extension = 0;
                    case.r.set_address_register(3, ea);
                }
                Family::Indexed => {
                    case.extension = 0x4800;
                    case.r.d[4] = 0;
                    case.r.set_address_register(3, ea);
                }
                Family::AbsoluteWord => {
                    case.r.pc = 0x4000;
                    case.extension = (case.r.pc + overlap) as u16;
                }
                Family::AbsoluteLong => {
                    case.extension = (ea >> 16) as u16;
                    case.destination_low = ea as u16;
                }
            }
            completed(case, true);
        }
    }
}
