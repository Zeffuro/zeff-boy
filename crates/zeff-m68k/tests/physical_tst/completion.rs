use super::*;

pub(super) fn values(size: Size) -> [u32; 4] {
    if size == Size::Long {
        [0, 0x0001_0000, 0x0000_8000, 0x8000_0001]
    } else {
        [0, 1, 0x8000, 0xffff]
    }
}

pub(super) fn inputs(case: Case, bus: &BusDevice) {
    let actual = oracle::address(case, &bus.words);
    let base = case.r.address_register(usize::from(case.register)).unwrap();
    let expected = match case.form.ea {
        Ea::Indirect | Ea::Post => base,
        Ea::Pre => base.wrapping_sub(case.form.stride()),
        Ea::Displacement => (i64::from(base) + i64::from(case.extension as i16)) as u32,
        Ea::Index => {
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
fn every_84_encoding_completes_in_both_banks_all_ccr_four_values_and_both_wait_choices() {
    let mut encodings = 0;
    let mut rows = 0;
    for form in forms() {
        for register in 0..if form.variable() { 8 } else { 1 } {
            encodings += 1;
            for supervisor in [false, true] {
                for ccr in 0..32 {
                    for value in values(form.size) {
                        for waited in [false, true] {
                            let mut case = Case::new(form);
                            case.r = registers(supervisor, ccr);
                            case.register = register;
                            case.value = value;
                            let (_, bus) = case.seed();
                            inputs(case, &bus);
                            oracle::completed(case, waited);
                            rows += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(forms().len(), 14);
    assert_eq!(encodings, 84);
    assert_eq!(rows, 43_008);
}

#[test]
fn literal_all_mode_source_and_extension_pc_table_preserves_old_sr_and_original_ird() {
    let table = [
        (Ea::Indirect, vec![], 2, 2),
        (Ea::Post, vec![], 2, 2),
        (Ea::Pre, vec![], 4, 2),
        (Ea::Displacement, vec![2], 2, 2),
        (Ea::Index, vec![2], 2, 2),
        (Ea::AbsoluteWord, vec![4], 4, 4),
        (Ea::AbsoluteLong, vec![2, 6], 6, 6),
    ];
    for (ea, extensions, word_pc, long_pc) in table {
        for size in [Size::Word, Size::Long] {
            for supervisor in [false, true] {
                let mut case = Case::new(Form { size, ea });
                case.r = registers(supervisor, 31);
                let (_, bus) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                for (i, offset) in extensions.iter().enumerate() {
                    assert_eq!(plan[i].internal_pc, case.r.pc.wrapping_add(*offset));
                    assert_eq!(plan[i].access.kind, AccessKind::Prefetch);
                }
                for t in &plan[..plan.len() - 1] {
                    assert_eq!(t.ird, case.form.opcode(case.register));
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
                        if supervisor {
                            FunctionCode::SupervisorData
                        } else {
                            FunctionCode::UserData
                        }
                    );
                }
                let last = plan.last().unwrap();
                assert_eq!(last.ird, case.next);
                assert_eq!(
                    last.internal_pc,
                    case.r.pc.wrapping_add(case.form.length() + 2)
                );
                assert_eq!(
                    last.registers.status_register(),
                    oracle::flags(
                        case.r.status_register(),
                        size,
                        oracle::value(case, &bus.words)
                    )
                );
            }
        }
    }
}

#[test]
fn every_index_selector_width_scale_bits_signed_offset_and_bank_uses_original_register_value() {
    let mut rows = 0;
    for size in [Size::Word, Size::Long] {
        for supervisor in [false, true] {
            for selector in 0..16 {
                for long in [false, true] {
                    for scale in 0..4 {
                        for offset in [0x70, 0x90] {
                            let mut case = Case::new(Form {
                                size,
                                ea: Ea::Index,
                            });
                            case.r = registers(supervisor, 31);
                            case.register = 7;
                            case.extension =
                                selector << 12 | if long { 0x800 } else { 0 } | scale << 9 | offset;
                            let (_, bus) = case.seed();
                            let raw = if selector < 8 {
                                case.r.d[usize::from(selector)]
                            } else {
                                case.r.address_register(usize::from(selector - 8)).unwrap()
                            };
                            let index = if long {
                                raw
                            } else {
                                i32::from(raw as i16) as u32
                            };
                            let expected = case
                                .r
                                .address_register(7)
                                .unwrap()
                                .wrapping_add(index)
                                .wrapping_add_signed(i32::from(offset as u8 as i8));
                            assert_eq!(oracle::address(case, &bus.words), expected);
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
    assert_eq!(rows, 1024);
}
