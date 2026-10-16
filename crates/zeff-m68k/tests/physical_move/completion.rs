use super::*;

#[test]
fn every_encoding_completes_with_exact_lanes_banks_queue_effects_and_owned_waits() {
    let mut encodings = [false; 384];
    for (form_index, form) in FORMS.into_iter().enumerate() {
        for dn in 0..8 {
            for an in 0..8 {
                for supervisor in [false, true] {
                    for old_ccr in [15, 31] {
                        for waits in [false, true] {
                            for lane in 0..if form.size == Size::Byte { 2 } else { 1 } {
                                let value = match form.size {
                                    Size::Byte => 0xa5,
                                    Size::Word => 0x807f,
                                    Size::Long => 0x1234_807f,
                                };
                                let address = 0xabcd_6000 + lane;
                                let (mut cpu, mut bus, initial) = seed(
                                    form,
                                    registers(supervisor, old_ccr),
                                    dn,
                                    an,
                                    value,
                                    address,
                                );
                                if waits {
                                    bus.elapsed = [7, 11, 13];
                                }
                                let plan = oracle::plan(form, initial, dn, an, value, 0x7201);
                                let expected_memory = oracle::memory(bus.words.clone(), &plan);
                                let clocks = bus.elapsed[..form.count()]
                                    .iter()
                                    .map(|&n| u64::from(n))
                                    .sum::<u64>();
                                let result = cpu.step_physical_instruction(&mut bus).unwrap();
                                let mut expected = plan.last().unwrap().registers;
                                expected.pc = initial.pc + 2;
                                assert_eq!(cpu.registers, expected);
                                assert_eq!(cpu.queued_words(), Some([0x7201, 0x7480]));
                                assert_eq!(bus.words, expected_memory);
                                assert_eq!(bus.events, oracle::events(&plan, &bus.elapsed, 37));
                                assert_eq!(
                                    result,
                                    PhysicalInstructionStep::Instruction(
                                        zeff_m68k::timing::TimedStep {
                                            opcode: form.opcode(dn, an),
                                            clocks,
                                            exception: None
                                        }
                                    )
                                );
                                assert_eq!(cpu.clock().0, 37 + clocks);
                                assert_eq!(cpu.clock().0, bus.clock);
                                assert_eq!(bus.entry_count, 0);
                                assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
                                encodings[form_index * 64 + usize::from(dn * 8 + an)] = true;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(encodings.into_iter().filter(|seen| *seen).count(), 384);
}

#[test]
fn wrapping_long_operands_preserve_full_logical_addresses_and_word_order() {
    for write in [false, true] {
        let form = Form {
            size: Size::Long,
            write,
        };
        let (mut cpu, mut bus, initial) =
            seed(form, registers(false, 31), 3, 2, 0x8000_0001, 0xffff_fffe);
        let plan = oracle::plan(form, initial, 3, 2, 0x8000_0001, 0x7201);
        let expected_memory = oracle::memory(bus.words.clone(), &plan);
        cpu.step_physical_instruction(&mut bus).unwrap();
        assert_eq!(bus.events, oracle::events(&plan, &bus.elapsed, 37));
        assert_eq!(plan[0].access.logical_address, 0xffff_fffe);
        assert_eq!(plan[0].access.bus_address, 0x00ff_fffe);
        assert_eq!(plan[1].access.logical_address, 0);
        assert_eq!(plan[1].access.bus_address, 0);
        assert_eq!(bus.words, expected_memory);
        let mut expected = plan[2].registers;
        expected.pc = initial.pc + 2;
        assert_eq!(cpu.registers, expected);
    }
}

#[test]
fn operand_aliases_sample_memory_while_retained_prefetch_remains_authoritative() {
    for form in FORMS {
        let mut initial = registers(false, 31);
        initial.pc = 0x2345_2000;
        let address = initial.pc + 2;
        let value = if form.write { 0x6008_4e71 } else { 0x7480_4e71 };
        let (mut cpu, mut bus, initial) = seed(form, initial, 1, 2, value, address);
        let plan = oracle::plan(form, initial, 1, 2, value, 0x7201);
        let expected_memory = oracle::memory(bus.words.clone(), &plan);
        let next = expected_memory[&((initial.pc + 4) & BUS_ADDRESS_MASK)];
        cpu.step_physical_instruction(&mut bus).unwrap();
        let mut expected = plan.last().unwrap().registers;
        if !form.write && form.size == Size::Long {
            expected.d[1] = 0x7480_7480;
            expected.set_status_register(oracle::flags(
                initial.status_register(),
                Size::Long,
                0x7480_7480,
            ));
        }
        expected.pc += 2;
        assert_eq!(cpu.registers, expected);
        assert_eq!(cpu.queued_words(), Some([0x7201, next]));
        assert_eq!(bus.words, expected_memory);
        assert_eq!(bus.events, oracle::events(&plan, &bus.elapsed, 37));
    }
}
