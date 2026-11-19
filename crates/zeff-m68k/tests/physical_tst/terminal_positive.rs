use super::*;

fn read_access(address: u32, program: bool, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & BUS_ADDRESS_MASK & !1,
        function_code: if program {
            FunctionCode::UserProgram
        } else {
            FunctionCode::UserData
        },
        kind: if program {
            AccessKind::Prefetch
        } else {
            AccessKind::Data
        },
        lanes,
    }
}

fn word(memory: &BTreeMap<u32, u16>, address: u32) -> u16 {
    memory
        .get(&(address & BUS_ADDRESS_MASK & !1))
        .copied()
        .unwrap_or(0x4e71)
}

pub(super) fn complete(cpu: &mut PrefetchCpu, bus: &mut BusDevice, opcode: u16) {
    let initial = cpu.registers;
    let memory = bus.words.clone();
    let retained = cpu.queued_words().unwrap()[1];
    let mut r = initial;
    let mut events = Vec::new();
    let mut clock = 37;
    let mut reads = 0;
    let mut anchor = initial.pc.wrapping_add(2);
    let mut next = retained;
    let mode = opcode >> 3 & 7;
    let register = usize::from(opcode & 7);
    let size = opcode >> 6 & 3;
    let (value, mask, sign) = if mode == 0 {
        let (mask, sign) = match size {
            0 => (255, 128),
            1 => (65535, 32768),
            _ => (u32::MAX, 0x8000_0000),
        };
        (initial.d[register], mask, sign)
    } else {
        assert_eq!(size, 0);
        let base = initial.address_register(register).unwrap();
        let stride = if register == 7 { 2 } else { 1 };
        let mut address = match mode {
            4 => base.wrapping_sub(stride),
            5 => base.wrapping_add_signed(i32::from(retained as i16)),
            6 => {
                let index_register = usize::from(retained >> 12 & 7);
                let index = if retained & 0x8000 == 0 {
                    initial.d[index_register]
                } else {
                    initial.address_register(index_register).unwrap()
                };
                let index = if retained & 0x0800 == 0 {
                    index as i16 as i32 as u32
                } else {
                    index
                };
                base.wrapping_add(index)
                    .wrapping_add_signed(i32::from(retained as u8 as i8))
            }
            7 if register == 0 => retained as i16 as i32 as u32,
            7 => u32::from(retained) << 16,
            _ => base,
        };
        if matches!(mode, 4 | 6) {
            events.push(Event::Idle(clock, 2));
            clock += 2;
        }
        let extensions = if mode < 5 {
            0
        } else if mode == 7 && register == 1 {
            2
        } else {
            1
        };
        for at in 0..extensions {
            let logical = initial.pc.wrapping_add(4 + 2 * at);
            events.push(Event::Read(
                read_access(logical, true, ByteLanes::Both),
                clock,
            ));
            next = word(&memory, logical);
            if mode == 7 && register == 1 && at == 0 {
                address |= u32::from(next);
            }
            clock += 4;
            reads += 1;
        }
        anchor = anchor.wrapping_add(2 * extensions);
        if mode == 3 {
            r.set_address_register(register, base.wrapping_add(stride));
        } else if mode == 4 {
            r.set_address_register(register, address);
        }
        let lanes = if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        };
        events.push(Event::Read(read_access(address, false, lanes), clock));
        let operand = word(&memory, address);
        let value = if address & 1 == 0 {
            operand >> 8
        } else {
            operand & 255
        };
        clock += 4;
        reads += 1;
        (u32::from(value), 255, 128)
    };
    r.set_status_register(
        (initial.status_register() & !15)
            | if value & mask == 0 { 4 } else { 0 }
            | if value & sign != 0 { 8 } else { 0 },
    );
    let fetched_address = anchor.wrapping_add(2);
    events.push(Event::Read(
        read_access(fetched_address, true, ByteLanes::Both),
        clock,
    ));
    clock += 4;
    reads += 1;
    r.pc = anchor;
    let expected =
        PrefetchCpu::from_prefetched(r, [next, word(&memory, fetched_address)], CpuClocks(clock))
            .unwrap();
    assert_eq!(
        cpu.step_physical_instruction(bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode,
                clocks: clock - 37,
                exception: None
            }
        ))
    );
    assert_eq!(*cpu, expected);
    assert_eq!(bus.words, memory);
    assert_eq!(bus.events, events);
    assert_eq!(bus.clock, clock);
    assert_eq!(bus.count, reads);
    assert_eq!(bus.elapsed, [4; 5]);
    assert_eq!(bus.failure, None);
    assert_eq!(bus.detection, None);
    assert_eq!(bus.entry_count, 0);
    assert_eq!(bus.entry_failure, None);
    assert_eq!(bus.mutate_read, None);
}
