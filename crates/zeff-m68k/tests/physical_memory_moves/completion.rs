use super::*;

pub(super) fn completed(case: Case, waited: bool) {
    let (cpu, bus, case) = case.seed();
    execute(cpu, bus, case, waited);
}

fn execute(mut cpu: PrefetchCpu, mut bus: BusDevice, case: Case, waited: bool) {
    if waited {
        bus.elapsed = [7, 11, 5, 13, 9, 6, 8];
    }
    let initial = bus.words.clone();
    let plan = oracle::plan(case, &initial);
    assert_eq!(plan.len(), case.form.count());
    let clocks = oracle::clocks(case, &bus.elapsed, plan.len(), true);
    let next = oracle::next(case, &initial);
    let fetched = oracle::fetched(initial.clone(), &plan);
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: case.form.opcode(case.source, case.destination),
                clocks,
                exception: None
            }
        ))
    );
    let mut expected = plan.last().unwrap().registers;
    expected.pc = case.r.pc.wrapping_add(case.form.length());
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), Some([next, fetched]));
    assert_eq!(bus.words, oracle::memory(initial, &plan));
    assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
    assert_eq!(bus.count, plan.len());
    assert_eq!(bus.entry_count, 0);
    assert_eq!(cpu.clock().0, case.start + clocks);
    assert_eq!(cpu.clock().0, bus.clock);
    assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
    assert!(!cpu.trace_pending());
}

#[test]
fn all_3168_encodings_cover_banks_ccr_values_waits_and_both_byte_lanes() {
    let mut encodings = std::collections::BTreeSet::new();
    let mut checked = 0;
    assert_eq!(forms().len(), 81);
    for form in forms() {
        for source in 0..if form.variable() { 8 } else { 1 } {
            for destination in 0..8 {
                assert!(encodings.insert(form.opcode(source, destination)));
                for supervisor in [false, true] {
                    for ccr in 0..32 {
                        for value in [0, 0x0001_0000, 0x0000_8000, 0x8000_0001] {
                            for waited in [false, true] {
                                for parity in 0..if form.size == Size::Byte { 2 } else { 1 } {
                                    let mut case = Case::new(form);
                                    case.r = registers(supervisor, ccr);
                                    case.source = source;
                                    case.destination = destination;
                                    case.value = value;
                                    if form.variable() {
                                        let old =
                                            case.r.address_register(usize::from(source)).unwrap();
                                        case.r.set_address_register(
                                            usize::from(source),
                                            old | parity,
                                        );
                                    } else if form.source == Source::AbsoluteLong {
                                        case.source_low |= parity as u16;
                                    } else {
                                        case.extension ^= parity as u16;
                                    }
                                    let old =
                                        case.r.address_register(usize::from(destination)).unwrap();
                                    case.r.set_address_register(
                                        usize::from(destination),
                                        old | parity,
                                    );
                                    let (cpu, bus, _) = case.seed();
                                    let mask = match form.size {
                                        Size::Byte => 255,
                                        Size::Word => 65535,
                                        Size::Long => u32::MAX,
                                    };
                                    assert_eq!(oracle::value(case, &bus.words), value & mask);
                                    execute(cpu, bus, case, waited);
                                    checked += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(encodings.len(), 3168);
    assert_eq!(checked, 2_162_688);
}

#[test]
fn source_destination_update_aliases_preserve_original_data_in_both_banks() {
    let mut checked = 0;
    for size in [Size::Byte, Size::Word, Size::Long] {
        for source in [Source::Post, Source::Pre] {
            for destination in [Destination::Post, Destination::Pre] {
                for register in [0, 7] {
                    for supervisor in [false, true] {
                        for value in [0x80, 0xff, 0x8000_0001, 0x0001_0000] {
                            let mut case = Case::new(Form {
                                size,
                                source,
                                destination,
                            });
                            case.r = registers(supervisor, 31);
                            case.source = register;
                            case.destination = register;
                            case.value = value;
                            completed(case, true);
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checked, 192);
}

#[test]
fn every_index_register_width_space_and_ignored_scale_uses_old_source_state() {
    let mut checked = 0;
    for size in [Size::Byte, Size::Word, Size::Long] {
        for source in [Source::Indexed, Source::PcIndexed] {
            for index in 0..8 {
                for address_index in [false, true] {
                    for long in [false, true] {
                        for supervisor in [false, true] {
                            for scale in [0, 0x600] {
                                let mut case = Case::new(Form {
                                    size,
                                    source,
                                    destination: Destination::Pre,
                                });
                                case.r = registers(supervisor, 31);
                                case.source = index;
                                case.destination = index;
                                case.extension = index << 12
                                    | if address_index { 0x8000 } else { 0 }
                                    | if long { 0x800 } else { 0 }
                                    | scale
                                    | 0xf0;
                                completed(case, true);
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checked, 768);
}

#[test]
fn full_logical_pc_and_source_destination_addresses_wrap_with_24_bit_bus_aliases() {
    let mut checked = 0;
    for form in forms() {
        let mask = match form.size {
            Size::Byte => 255,
            Size::Word => 65535,
            Size::Long => u32::MAX,
        };
        for pc in [0xffff_fffa, 0x00ff_fffc] {
            let mut case = Case::new(form);
            case.r.pc = pc;
            case.source = 0;
            case.destination = 1;
            case.r.a[0] = if form.source == Source::Pre {
                2
            } else {
                0xffff_fffe
            };
            case.r.a[1] = if form.destination == Destination::Pre {
                2
            } else {
                0x00ff_fffe
            };
            case.extension = match form.source {
                Source::AbsoluteWord => 0xfffe,
                Source::AbsoluteLong => 0xffff,
                Source::Indexed | Source::PcIndexed => 0x48fe,
                _ => 0xfffe,
            };
            case.source_low = if form.source == Source::AbsoluteLong {
                0x8000
            } else {
                0xfffe
            };
            case.r.d[4] = 0;
            if form.source == Source::AbsoluteLong {
                let (_, bus, _) = case.seed();
                assert_eq!(oracle::source_address(case, &bus.words), 0xffff_8000);
                assert_eq!(oracle::value(case, &bus.words), case.value & mask);
            }
            completed(case, false);
            checked += 1;
        }
        if form.source == Source::AbsoluteLong {
            let mut case = Case::new(form);
            case.extension = 0xffff;
            case.source_low = 0xfffe;
            let (_, bus, _) = case.seed();
            assert_eq!(oracle::source_address(case, &bus.words), 0xffff_fffe);
            assert_eq!(oracle::value(case, &bus.words), case.value & mask);
            completed(case, false);
            checked += 1;
        }
    }
    assert_eq!(checked, 171);
}

#[test]
fn actual_source_extension_low_data_high_low_next_and_refill_are_sampled_from_ram() {
    for form in forms() {
        let case = Case::new(form);
        let (cpu, bus, _) = case.seed();
        let source = oracle::source_address(case, &bus.words);
        let mut addresses = vec![source, case.r.pc.wrapping_add(form.length() + 2)];
        if form.size == Size::Long {
            addresses.push(source.wrapping_add(2));
        }
        if form.extensions() != 0 {
            addresses.push(case.r.pc.wrapping_add(form.length()));
        }
        for address in addresses {
            let mut bus = BusDevice::with_clock(case.start);
            bus.words = case.seed().1.words;
            bus.mutate_read = Some((address, 0x7602));
            let mut expected_words = bus.words.clone();
            expected_words.insert(address & BUS_ADDRESS_MASK & !1, 0x7602);
            let plan = oracle::plan(case, &expected_words);
            let mut actual_cpu = cpu.clone();
            let next = oracle::next(case, &expected_words);
            let fetched = oracle::fetched(expected_words.clone(), &plan);
            assert!(actual_cpu.step_physical_instruction(&mut bus).is_ok());
            let mut r = plan.last().unwrap().registers;
            r.pc = case.r.pc.wrapping_add(form.length());
            assert_eq!(actual_cpu.registers, r);
            assert_eq!(actual_cpu.queued_words(), Some([next, fetched]));
            assert_eq!(bus.words, oracle::memory(expected_words, &plan));
            assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
            assert_eq!(actual_cpu.clock().0, bus.clock);
        }
        if form.source == Source::AbsoluteLong {
            let (mut cpu, mut bus, _) = case.seed();
            let address = case.r.pc.wrapping_add(4);
            bus.mutate_read = Some((address, 0x9000));
            bus.operand(
                u32::from(case.extension) << 16 | 0x9000,
                form.size,
                0x0001_8000,
            );
            let mut shadow = bus.words.clone();
            shadow.insert(address & BUS_ADDRESS_MASK & !1, 0x9000);
            let plan = oracle::plan(case, &shadow);
            assert!(cpu.step_physical_instruction(&mut bus).is_ok());
            assert_eq!(bus.words, oracle::memory(shadow, &plan));
            let mut r = plan.last().unwrap().registers;
            r.pc = case.r.pc.wrapping_add(form.length());
            assert_eq!(cpu.registers, r);
        }
    }
}

#[test]
fn source_and_destination_overlap_and_code_writes_preserve_retained_n_and_early_f() {
    for form in forms() {
        for code in [false, true] {
            let mut case = Case::new(form);
            case.source = 0;
            case.destination = 1;
            let target = if code {
                case.r.pc.wrapping_add(form.length())
            } else {
                case.source_address(case.source_low)
            };
            case.r.a[1] = if form.destination == Destination::Pre {
                target.wrapping_add(form.stride(1))
            } else {
                target
            };
            completed(case, true);
        }
    }
}
