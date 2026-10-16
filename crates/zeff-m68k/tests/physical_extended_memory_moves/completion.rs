use super::*;

pub(super) fn completed(case: Case, waited: bool) {
    let (cpu, bus, case) = case.seed();
    execute(cpu, bus, case, waited);
}

fn execute(mut cpu: PrefetchCpu, mut bus: BusDevice, case: Case, waited: bool) {
    if waited {
        bus.elapsed = [7, 11, 5, 13, 9, 6, 8, 10, 12];
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
fn all_2376_encodings_cover_banks_ccr_values_waits_and_both_byte_lanes() {
    let mut encodings = std::collections::BTreeSet::new();
    let mut checked = 0;
    assert_eq!(forms().len(), 108);
    for form in forms() {
        for source in 0..if form.variable() { 8 } else { 1 } {
            for destination in 0..if form.destination_variable() { 8 } else { 1 } {
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
                                    match form.destination {
                                        Destination::Displacement | Destination::Indexed => {
                                            let old = case
                                                .r
                                                .address_register(usize::from(destination))
                                                .unwrap();
                                            case.r.set_address_register(
                                                usize::from(destination),
                                                old | parity,
                                            );
                                        }
                                        Destination::AbsoluteWord => {
                                            case.destination_extension ^= parity as u16
                                        }
                                        Destination::AbsoluteLong => {
                                            case.destination_low ^= parity as u16
                                        }
                                    }
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
    assert_eq!(encodings.len(), 2376);
    assert_eq!(checked, 1_622_016);
}

#[test]
fn source_destination_update_aliases_preserve_original_data_in_both_banks() {
    let mut checked = 0;
    for size in [Size::Byte, Size::Word, Size::Long] {
        for source in [Source::Post, Source::Pre] {
            for destination in [Destination::Displacement, Destination::Indexed] {
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
                                    destination: Destination::Indexed,
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
            case.r.a[0] = 0x6000;
            case.r.a[1] = 0xa000;
            case.r.d[4] = if form.source == Source::PcIndexed {
                0x6000
            } else {
                0
            };
            case.r.d[5] = 0;
            case.extension = match form.source {
                Source::PcDisplacement => 0x6000,
                Source::Indexed | Source::PcIndexed => 0x4800,
                Source::AbsoluteWord => 0x9000,
                Source::AbsoluteLong => 0xabcd,
                _ => 0,
            };
            case.destination_extension = match form.destination {
                Destination::Displacement => 0,
                Destination::Indexed => 0x5800,
                Destination::AbsoluteWord => 0xa000,
                Destination::AbsoluteLong => 0x5678,
            };
            let (_, bus, _) = case.seed();
            let source = oracle::source_address(case, &bus.words);
            let destination = oracle::destination_address(case, &bus.words);
            assert_eq!(source, case.source_address(case.source_low));
            assert_eq!(destination, case.destination_address());
            if form.size != Size::Byte {
                assert_eq!((source | destination) & 1, 0);
            }
            assert_eq!(oracle::value(case, &bus.words), case.value & mask);
            assert!(
                bus.words
                    .contains_key(&(pc.wrapping_add(form.length() + 2) & BUS_ADDRESS_MASK & !1))
            );
            completed(case, false);
            checked += 1;
        }
        let mut case = Case::new(form);
        case.r.pc = 0xffff_b000;
        case.source = 0;
        case.destination = 1;
        case.r.a[0] = if form.source == Source::Pre {
            0xffff_fffe_u32.wrapping_add(form.stride(0))
        } else {
            0xffff_fffe
        };
        case.r.a[1] = 0x00ff_fffe;
        case.r.d[4] = if form.source == Source::PcIndexed {
            0x4ffc
        } else {
            0
        };
        case.r.d[5] = 0;
        case.extension = match form.source {
            Source::AbsoluteWord => 0xfffe,
            Source::AbsoluteLong => 0xffff,
            Source::PcDisplacement => 0x4ffc,
            Source::Indexed | Source::PcIndexed => 0x4800,
            _ => 0,
        };
        case.source_low = 0xfffe;
        case.destination_extension = match form.destination {
            Destination::Displacement => 0,
            Destination::Indexed => 0x5800,
            Destination::AbsoluteWord => 0xfffe,
            Destination::AbsoluteLong => 0x00ff,
        };
        case.destination_low = 0xfffe;
        let (_, bus, _) = case.seed();
        assert_eq!(oracle::source_address(case, &bus.words), 0xffff_fffe);
        assert_eq!(
            oracle::destination_address(case, &bus.words),
            if form.destination == Destination::AbsoluteWord {
                0xffff_fffe
            } else {
                0x00ff_fffe
            }
        );
        assert_eq!(oracle::value(case, &bus.words), case.value & mask);
        completed(case, false);
        checked += 1;
    }
    assert_eq!(checked, 324);
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
        addresses.push(case.r.pc.wrapping_add(form.length()));
        if form.extensions() != 0 {
            addresses.push(case.r.pc.wrapping_add(form.cursor()));
        }
        if form.destination == Destination::AbsoluteLong {
            addresses.push(case.r.pc.wrapping_add(form.cursor() + 2));
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
fn source_and_destination_overlap_and_code_writes_sample_n_at_the_correct_time() {
    for form in forms() {
        for code in [false, true] {
            let mut case = Case::new(form);
            case.r.pc = 0x4000;
            case.source = 0;
            case.destination = 1;
            case.r.a[0] = 0x6000;
            case.r.d[4] = if form.source == Source::PcIndexed {
                0x2000
            } else {
                0
            };
            case.source_low = 0x6000;
            case.extension = match form.source {
                Source::AbsoluteLong => 0,
                Source::AbsoluteWord => 0x6000,
                Source::PcDisplacement => 0x2000,
                Source::Indexed | Source::PcIndexed => 0x4800,
                _ => 0,
            };
            let target = if code {
                case.r.pc.wrapping_add(form.length())
            } else {
                case.source_address(case.source_low)
            };
            if form.destination == Destination::Indexed {
                case.destination_extension = 0x5800;
                case.r.d[5] = 0;
                case.r.a[1] = target;
            } else {
                case.target(target);
            }
            let (_, bus, _) = case.seed();
            assert_eq!(oracle::destination_address(case, &bus.words), target);
            completed(case, true);
        }
    }
}

#[test]
fn every_destination_index_selector_width_space_scale_and_updated_source_alias_is_retained() {
    let mut checked = 0;
    for size in [Size::Byte, Size::Word, Size::Long] {
        for source in [Source::Post, Source::Pre] {
            for selector in 0..8 {
                for address_index in [false, true] {
                    for long in [false, true] {
                        for supervisor in [false, true] {
                            for scale in [0, 0x600] {
                                let mut case = Case::new(Form {
                                    size,
                                    source,
                                    destination: Destination::Indexed,
                                });
                                case.r = registers(supervisor, 31);
                                case.source = selector;
                                case.destination = selector;
                                case.destination_extension = selector << 12
                                    | 0xf0
                                    | scale
                                    | if address_index { 0x8000 } else { 0 }
                                    | if long { 0x800 } else { 0 };
                                let old = 0x8000;
                                case.r.set_address_register(usize::from(selector), old);
                                case.r.d[usize::from(selector)] = 0xffff_8000;
                                let (_, bus, _) = case.seed();
                                let step = case.form.stride(selector);
                                let updated = if source == Source::Post {
                                    old + step
                                } else {
                                    old - step
                                };
                                let raw = if address_index { updated } else { 0xffff_8000 };
                                let index = if long {
                                    raw
                                } else {
                                    i32::from(raw as i16) as u32
                                };
                                assert_eq!(
                                    oracle::destination_address(case, &bus.words),
                                    updated.wrapping_add(index).wrapping_sub(16)
                                );
                                completed(case, true);
                                for at in 0..case.form.count() {
                                    capture::failed(case, at, Failure::Boundary);
                                }
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
fn source_updates_feed_displacement_base_and_byte_a7_stride_in_both_banks() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        for source in [Source::Post, Source::Pre] {
            for register in [0, 7] {
                for supervisor in [false, true] {
                    let mut case = Case::new(Form {
                        size,
                        source,
                        destination: Destination::Displacement,
                    });
                    case.r = registers(supervisor, 31);
                    case.source = register;
                    case.destination = register;
                    case.r.set_address_register(usize::from(register), 0x8000);
                    case.destination_extension = 0xfff0;
                    let stride = case.form.stride(register);
                    assert_eq!(
                        stride,
                        match size {
                            Size::Byte if register == 0 => 1,
                            Size::Long => 4,
                            _ => 2,
                        }
                    );
                    let expected = if source == Source::Post {
                        0x8000 + stride
                    } else {
                        0x8000 - stride
                    };
                    let (_, bus, _) = case.seed();
                    assert_eq!(oracle::destination_address(case, &bus.words), expected - 16);
                    completed(case, true);
                    for at in 0..case.form.count() {
                        capture::failed(case, at, Failure::Boundary);
                    }
                }
            }
        }
    }
}
