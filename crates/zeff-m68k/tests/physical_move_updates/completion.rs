use super::*;

fn complete(
    form: Form,
    pair: (u16, u16),
    mut cpu: PrefetchCpu,
    mut bus: Device,
    initial: Registers,
    value: u32,
) {
    let (dn, an) = pair;
    let plan = oracle::plan(form, initial, dn, an, value, 0x7201);
    let next = oracle::fetched(bus.words.clone(), &plan);
    let memory = oracle::memory(bus.words.clone(), &plan);
    let clocks = u64::from(form.idle())
        + bus.elapsed[..form.count()]
            .iter()
            .map(|&n| u64::from(n))
            .sum::<u64>();
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: form.opcode(dn, an),
                clocks,
                exception: None
            }
        ))
    );
    let mut registers = plan.last().unwrap().registers;
    registers.pc = initial.pc.wrapping_add(2);
    assert_eq!(cpu.registers, registers);
    assert_eq!(cpu.queued_words(), Some([0x7201, next]));
    assert_eq!(bus.events, oracle::events(form, &plan, &bus.elapsed, 37));
    assert_eq!(bus.words, memory);
    assert_eq!(cpu.clock().0, 37 + clocks);
    assert_eq!(cpu.clock().0, bus.clock);
    assert_eq!(bus.entry_count, 0);
    assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
}

#[test]
fn all_encodings_preserve_banks_x_lanes_and_full_registers_with_owned_waits() {
    let mut seen = [false; 768];
    let mut cases = 0;
    for (index, form) in FORMS.into_iter().enumerate() {
        for dn in 0..8 {
            for an in 0..8 {
                for supervisor in [false, true] {
                    for ccr in [15, 31] {
                        for waits in [false, true] {
                            for lane in 0..if form.size == Size::Byte { 2 } else { 1 } {
                                let value = match form.size {
                                    Size::Byte => 0xa5,
                                    Size::Word => 0x807f,
                                    Size::Long => 0x1234_807f,
                                };
                                let (cpu, mut bus, initial) = seed(
                                    form,
                                    registers(supervisor, ccr),
                                    dn,
                                    an,
                                    value,
                                    0xabcd_6000 + lane,
                                );
                                if waits {
                                    bus.elapsed = [7, 11, 13];
                                }
                                complete(form, (dn, an), cpu, bus, initial, value);
                                seen[index * 64 + usize::from(dn * 8 + an)] = true;
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(seen.into_iter().filter(|seen| *seen).count(), 768);
    assert_eq!(cases, 8192);
}

#[test]
fn byte_stack_updates_are_two_bytes_while_transfers_remain_single_lane() {
    for form in FORMS.into_iter().filter(|f| f.size == Size::Byte) {
        for supervisor in [false, true] {
            for lane in [0, 1] {
                let (mut cpu, mut bus, initial) = seed(
                    form,
                    registers(supervisor, 31),
                    0,
                    7,
                    0x80,
                    0xabcd_6000 + lane,
                );
                cpu.step_physical_instruction(&mut bus).unwrap();
                let expected = if form.pre() {
                    0xabcd_5ffe + lane
                } else {
                    0xabcd_6002 + lane
                };
                assert_eq!(cpu.registers.address_register(7), Some(expected));
                assert_eq!(
                    if supervisor {
                        cpu.registers.usp
                    } else {
                        cpu.registers.ssp
                    },
                    if supervisor { initial.usp } else { initial.ssp }
                );
                let data: Vec<_> = bus
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        Event::Read(access, _) | Event::Write(access, _, _)
                            if access.kind == AccessKind::Data =>
                        {
                            Some(*access)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(data.len(), 1);
                assert_eq!(
                    data[0].lanes,
                    if lane == 0 {
                        ByteLanes::Upper
                    } else {
                        ByteLanes::Lower
                    }
                );
            }
        }
    }
}

#[test]
fn wrapping_addresses_keep_logical_arithmetic_word_order_and_external_masking() {
    for form in FORMS {
        let old = if form.pre() {
            if form.size == Size::Long { 2 } else { 0 }
        } else {
            u32::MAX - u32::from(form.size != Size::Byte)
        };
        let value = if form.size == Size::Long {
            0x8000_0001
        } else {
            0x80
        };
        let (cpu, bus, initial) = seed(form, registers(false, 31), 3, 2, value, old);
        let plan = oracle::plan(form, initial, 3, 2, value, 0x7201);
        for transfer in &plan {
            assert_eq!(
                transfer.access.bus_address,
                transfer.access.logical_address & 0x00ff_fffe
            );
        }
        if form.size == Size::Long {
            let addresses: Vec<_> = plan
                .iter()
                .filter(|t| t.access.kind == AccessKind::Data)
                .map(|t| t.access.logical_address)
                .collect();
            assert_eq!(
                addresses,
                if form.route == Route::PreStore {
                    vec![0, 0xffff_fffe]
                } else {
                    vec![0xffff_fffe, 0]
                }
            );
        }
        complete(form, (3, 2), cpu, bus, initial, value);
    }
}

#[test]
fn code_aliasing_preserves_the_actual_early_fetch_and_original_retained_queue() {
    for form in FORMS {
        let initial = registers(false, 31);
        let operand = initial.pc + 2;
        let old_address = if form.pre() {
            operand + form.stride(2)
        } else {
            operand
        };
        let value = 0x6008_4e71;
        let (cpu, bus, initial) = seed(form, initial, 3, 2, value, old_address);
        let actual_value = if form.write() {
            value
        } else {
            match form.size {
                Size::Byte => u32::from(bus.sample(operand) >> 8),
                Size::Word => u32::from(bus.sample(operand)),
                Size::Long => {
                    (u32::from(bus.sample(operand)) << 16) | u32::from(bus.sample(operand + 2))
                }
            }
        };
        if form.route == Route::PreStore {
            assert_eq!(bus.sample(initial.pc + 4), 0x7480);
        }
        complete(form, (3, 2), cpu, bus, initial, actual_value);
    }
}
