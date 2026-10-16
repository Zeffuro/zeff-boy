use super::*;

pub(super) fn pairs(size: Size) -> [(u32, u32); 12] {
    if size == Size::Word {
        [
            (0, 0),
            (0, 2),
            (1, 0),
            (2, 2),
            (0x7fff, 0x8000_0000),
            (0x8000, 0x7fff_fffe),
            (0x8000, 0xffff_8000),
            (0xffff, 0x7fff_fffe),
            (0, 0x8000_0000),
            (0x8001, 0),
            (0x7ffe, 0),
            (0x7fff, 0),
        ]
    } else {
        [
            (0, 0),
            (0, 2),
            (1, 0),
            (2, 2),
            (0x7fff_fffe, 0x8000_0000),
            (0x8000_0000, 0x7fff_fffe),
            (0x8000_0000, 0x8000_0000),
            (u32::MAX, 0x7fff_fffe),
            (0, 0x8000_0000),
            (0x8000_0001, 0),
            (0x10000, 0),
            (0x7fff_ffff, 0),
        ]
    }
}

pub(super) fn pair(case: &mut Case, (source, destination): (u32, u32)) {
    case.value = source;
    case.expected_destination = Some(destination);
    let initial = if case.destination == case.register {
        match case.form.ea {
            Ea::Post => destination.wrapping_sub(case.form.stride()),
            Ea::Pre => destination.wrapping_add(case.form.stride()),
            _ => destination,
        }
    } else {
        destination
    };
    case.r
        .set_address_register(usize::from(case.destination), initial);
}

pub(super) fn compared_destination(case: Case) -> u32 {
    let initial = case
        .r
        .address_register(usize::from(case.destination))
        .unwrap();
    if case.destination == case.register {
        match case.form.ea {
            Ea::Post => initial.wrapping_add(case.form.stride()),
            Ea::Pre => initial.wrapping_sub(case.form.stride()),
            _ => initial,
        }
    } else {
        initial
    }
}

pub(super) fn inputs(case: Case, bus: &BusDevice) {
    let actual = oracle::address(case, &bus.words);
    let base = if matches!(case.form.ea, Ea::PcDisplacement | Ea::PcIndex) {
        case.r.pc.wrapping_add(2)
    } else {
        case.r.address_register(usize::from(case.register)).unwrap()
    };
    let expected = match case.form.ea {
        Ea::Indirect | Ea::Post => base,
        Ea::Pre => base.wrapping_sub(case.form.stride()),
        Ea::Displacement | Ea::PcDisplacement => {
            (i64::from(base) + i64::from(case.extension as i16)) as u32
        }
        Ea::Index | Ea::PcIndex => {
            let register = usize::from(case.extension >> 12 & 7);
            let raw = if case.extension & 0x8000 != 0 {
                case.r.address_register(register).unwrap()
            } else {
                case.r.d[register]
            };
            let index = if case.extension & 0x800 != 0 {
                i64::from(raw as i32)
            } else {
                i64::from(raw as i16)
            };
            (i64::from(base) + index + i64::from(case.extension as u8 as i8)) as u32
        }
        Ea::AbsoluteWord => i32::from(case.extension as i16) as u32,
        Ea::AbsoluteLong => u32::from(case.extension) << 16 | u32::from(case.low),
    };
    assert_eq!(actual, expected);
    assert_eq!(actual & 1, 0);
    if let Some(expected) = case.expected_destination {
        assert_eq!(compared_destination(case), expected);
        assert_eq!(expected & 1, 0);
    }
    assert_eq!(
        oracle::value(case, &bus.words),
        case.value
            & if case.form.size == Size::Long {
                u32::MAX
            } else {
                65535
            }
    );
}

#[test]
fn every_1408_encoding_completes_both_banks_all_ccr_twelve_pairs_and_both_wait_choices() {
    let mut encodings = 0;
    let mut rows = 0;
    for form in forms() {
        for register in 0..if form.variable() { 8 } else { 1 } {
            for destination in 0..8 {
                encodings += 1;
                for supervisor in [false, true] {
                    for ccr in 0..32 {
                        for operands in pairs(form.size) {
                            for waited in [false, true] {
                                let mut case = Case::new(form);
                                case.r = registers(supervisor, ccr);
                                case.register = register;
                                case.destination = destination;
                                pair(&mut case, operands);
                                let (_, bus) = case.seed();
                                inputs(case, &bus);
                                assert_eq!(compared_destination(case), operands.1);
                                oracle::completed(case, waited);
                                rows += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(forms().len(), 36);
    assert_eq!(encodings, 1408);
    assert_eq!(rows, 2_162_688);
}

#[test]
fn literal_word_sign_extension_and_high_carry_borrow_preserve_whole_status() {
    for (operation, source, destination, partial, result) in [
        (Operation::Add, 0xffff, 0x20000, 0x2ffff, 0x1ffff),
        (Operation::Subtract, 0xffff, 0x1fffe, 0x1ffff, 0x1ffff),
        (Operation::Add, 2, 0x1fffe, 0x10000, 0x20000),
        (Operation::Subtract, 2, 0x20000, 0x2fffe, 0x1fffe),
    ] {
        for ccr in 0..32 {
            let mut case = Case::new(Form {
                operation,
                size: Size::Word,
                ea: Ea::Indirect,
            });
            case.r = registers(false, ccr);
            case.r.a[5] = destination;
            case.value = source;
            let (_, bus) = case.seed();
            assert_eq!(oracle::arithmetic(case, &bus.words), (partial, result));
            assert!(
                oracle::plan(case, &bus.words)
                    .iter()
                    .all(|t| t.registers.status_register() == case.r.status_register())
            );
            oracle::completed(case, true);
        }
    }
}

#[test]
fn literal_all_mode_source_extension_pc_and_program_fc_table_keeps_old_sr_and_ird() {
    let table = [
        (Ea::Indirect, vec![], 2, 2),
        (Ea::Post, vec![], 2, 2),
        (Ea::Pre, vec![], 4, 2),
        (Ea::Displacement, vec![2], 2, 2),
        (Ea::Index, vec![2], 2, 2),
        (Ea::AbsoluteWord, vec![4], 4, 4),
        (Ea::AbsoluteLong, vec![2, 6], 6, 6),
        (Ea::PcDisplacement, vec![2], 2, 2),
        (Ea::PcIndex, vec![2], 2, 2),
    ];
    for (ea, extensions, word_pc, long_pc) in table {
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
                let mut case = Case::new(Form {
                    operation,
                    size,
                    ea,
                });
                case.r = registers(supervisor, 31);
                let (_, bus) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                for (i, offset) in extensions.iter().enumerate() {
                    assert_eq!(plan[i].internal_pc, case.r.pc.wrapping_add(*offset));
                    assert_eq!(plan[i].access.kind, AccessKind::Prefetch);
                }
                for t in &plan[..plan.len() - 1] {
                    assert_eq!(t.ird, case.opcode());
                    assert_eq!(t.registers.status_register(), case.r.status_register());
                }
                for t in &plan[case.form.extensions()..plan.len() - 1] {
                    assert_eq!(
                        t.internal_pc,
                        case.r
                            .pc
                            .wrapping_add(if size == Size::Long { long_pc } else { word_pc })
                    );
                    assert_eq!(t.access.kind, AccessKind::Data);
                    assert_eq!(
                        t.access.function_code,
                        match (supervisor, matches!(ea, Ea::PcDisplacement | Ea::PcIndex)) {
                            (false, false) => FunctionCode::UserData,
                            (true, false) => FunctionCode::SupervisorData,
                            (false, true) => FunctionCode::UserProgram,
                            (true, true) => FunctionCode::SupervisorProgram,
                        }
                    );
                }
                let last = plan.last().unwrap();
                assert_eq!(last.ird, case.next);
                assert_eq!(
                    last.internal_pc,
                    case.r.pc.wrapping_add(case.form.length() + 2)
                );
                assert_eq!(last.registers.status_register(), case.r.status_register());
            }
        }
    }
}

#[test]
fn every_index_and_pc_index_selector_width_ignored_scale_sign_and_compared_an_alias_uses_original_value()
 {
    let mut rows = 0;
    for ea in [Ea::Index, Ea::PcIndex] {
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
                for selector in 0..16 {
                    for long in [false, true] {
                        for ignored in 0..8 {
                            for offset in [0x70, 0x90] {
                                for operands in pairs(size) {
                                    let mut case = Case::new(Form {
                                        operation,
                                        size,
                                        ea,
                                    });
                                    case.r = registers(supervisor, 31);
                                    case.register = 7;
                                    case.destination = selector & 7;
                                    pair(&mut case, operands);
                                    let raw = if selector < 8 {
                                        case.r.d[usize::from(selector)]
                                    } else {
                                        case.r.address_register(usize::from(selector - 8)).unwrap()
                                    };
                                    let aligned_offset = offset | (raw & 1) as u16;
                                    case.extension = selector << 12
                                        | if long { 0x800 } else { 0 }
                                        | ignored << 8
                                        | aligned_offset;
                                    let (_, bus) = case.seed();
                                    let index = if long {
                                        raw
                                    } else {
                                        i32::from(raw as i16) as u32
                                    };
                                    let base = if ea == Ea::PcIndex {
                                        case.r.pc + 2
                                    } else {
                                        case.r.address_register(7).unwrap()
                                    };
                                    assert_eq!(
                                        oracle::address(case, &bus.words),
                                        base.wrapping_add(index).wrapping_add_signed(i32::from(
                                            aligned_offset as u8 as i8
                                        ))
                                    );
                                    inputs(case, &bus);
                                    oracle::completed(case, true);
                                    for at in 0..case.form.count() {
                                        capture::failed(case, at, Failure::Boundary);
                                    }
                                    rows += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(rows, 98_304);
}
